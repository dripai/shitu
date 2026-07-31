package com.dripai.shiping

import android.Manifest
import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.media.projection.MediaProjectionManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import com.dripai.shiping.recording.AudioMode
import com.dripai.shiping.recording.RecordingConfig
import com.dripai.shiping.recording.RecordingHistoryRepository
import com.dripai.shiping.recording.RecordingItem
import com.dripai.shiping.recording.RecordingService
import com.dripai.shiping.recording.RecordingStateStore
import com.dripai.shiping.ui.ShiPingApp
import com.dripai.shiping.ui.theme.ShiPingTheme

class MainActivity : ComponentActivity() {
    private var pendingConfig: RecordingConfig? = null
    private val historyRepository by lazy {
        RecordingHistoryRepository(applicationContext)
    }

    private val overlayPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
            val config = pendingConfig ?: return@registerForActivityResult
            if (Settings.canDrawOverlays(this)) {
                continueRecordingRequest(config)
            } else {
                pendingConfig = null
                RecordingStateStore.failed("悬浮窗权限被拒绝，无法显示录制状态")
            }
        }

    private val audioPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            val config = pendingConfig ?: return@registerForActivityResult
            if (granted) {
                launchProjectionConsent(config)
            } else {
                pendingConfig = null
                RecordingStateStore.failed("录音权限被拒绝，请选择无声音或在系统设置中授权")
            }
        }

    private val projectionLauncher =
        registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
            val config = pendingConfig
            pendingConfig = null

            if (
                config == null ||
                result.resultCode != Activity.RESULT_OK ||
                result.data == null
            ) {
                RecordingStateStore.failed("系统录屏授权被取消")
                return@registerForActivityResult
            }

            RecordingService.start(
                context = this,
                resultCode = result.resultCode,
                projectionData = result.data!!,
                config = config,
            )
        }

    private val notificationPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        if (
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
        }

        setContent {
            ShiPingTheme {
                ShiPingApp(
                    state = RecordingStateStore.state,
                    onStart = ::requestRecording,
                    onStop = { RecordingService.stop(this) },
                    onLoadRecordings = historyRepository::load,
                    canOpenRecording = ::canOpenRecording,
                    onOpenRecording = ::openRecording,
                )
            }
        }
    }

    private fun requestRecording(config: RecordingConfig) {
        if (RecordingStateStore.state.value.isActive) {
            return
        }

        if (!Settings.canDrawOverlays(this)) {
            pendingConfig = config
            RecordingStateStore.authorizing("正在等待悬浮窗权限")
            val intent = Intent(
                Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
                Uri.parse("package:$packageName"),
            )
            if (intent.resolveActivity(packageManager) == null) {
                pendingConfig = null
                RecordingStateStore.failed("当前系统没有可用的悬浮窗权限设置页面")
                return
            }
            overlayPermissionLauncher.launch(intent)
            return
        }

        continueRecordingRequest(config)
    }

    private fun continueRecordingRequest(config: RecordingConfig) {
        if (
            config.audioMode != AudioMode.None &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            pendingConfig = config
            audioPermissionLauncher.launch(Manifest.permission.RECORD_AUDIO)
            return
        }

        launchProjectionConsent(config)
    }

    private fun launchProjectionConsent(config: RecordingConfig) {
        pendingConfig = config
        RecordingStateStore.authorizing()
        val manager = getSystemService(MediaProjectionManager::class.java)
        projectionLauncher.launch(manager.createScreenCaptureIntent())
    }

    private fun canOpenRecording(recording: RecordingItem): Boolean =
        recordingViewIntent(recording).resolveActivity(packageManager) != null

    private fun openRecording(recording: RecordingItem): Boolean {
        return try {
            startActivity(recordingViewIntent(recording))
            true
        } catch (_: ActivityNotFoundException) {
            false
        } catch (_: SecurityException) {
            false
        }
    }

    private fun recordingViewIntent(recording: RecordingItem): Intent =
        Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(recording.uri, "video/mp4")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
}
