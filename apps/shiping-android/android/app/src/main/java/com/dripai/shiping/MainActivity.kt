package com.dripai.shiping

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import com.dripai.shiping.recording.AudioMode
import com.dripai.shiping.recording.RecordingConfig
import com.dripai.shiping.recording.RecordingService
import com.dripai.shiping.recording.RecordingStateStore
import com.dripai.shiping.ui.ShiPingApp
import com.dripai.shiping.ui.theme.ShiPingTheme

class MainActivity : ComponentActivity() {
    private var pendingConfig: RecordingConfig? = null

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
                )
            }
        }
    }

    private fun requestRecording(config: RecordingConfig) {
        if (RecordingStateStore.state.value.isActive) {
            return
        }

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
}
