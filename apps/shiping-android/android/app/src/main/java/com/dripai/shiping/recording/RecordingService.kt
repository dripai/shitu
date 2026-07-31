package com.dripai.shiping.recording

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.IBinder
import androidx.core.content.ContextCompat
import com.dripai.shiping.MainActivity
import com.dripai.shiping.R

class RecordingService : Service(), ScreenRecorder.Callbacks {
    private var recorder: ScreenRecorder? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> startRecording(intent)
            ACTION_STOP -> stopRecording()
        }
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        recorder?.stop()
        recorder = null
        super.onDestroy()
    }

    private fun startRecording(intent: Intent) {
        if (recorder != null) {
            return
        }

        val config = intent.toRecordingConfig()
        startTypedForeground(config.audioMode)

        val resultCode = intent.getIntExtra(EXTRA_RESULT_CODE, 0)
        val projectionData = intent.parcelableIntentExtra(EXTRA_PROJECTION_DATA)
        if (projectionData == null) {
            RecordingStateStore.failed("缺少系统录屏授权数据")
            stopSelf()
            return
        }

        try {
            val projectionManager = getSystemService(MediaProjectionManager::class.java)
            val projection = projectionManager.getMediaProjection(resultCode, projectionData)
            recorder = ScreenRecorder(
                context = applicationContext,
                projection = projection,
                config = config,
                callbacks = this,
            ).also(ScreenRecorder::start)
        } catch (error: Exception) {
            RecordingStateStore.failed("无法启动录屏：${error.readableMessage()}")
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }
    }

    private fun stopRecording() {
        val current = recorder
        if (current == null) {
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return
        }

        RecordingStateStore.finalizing(RecordingStateStore.state.value.elapsedMs)
        current.stop()
    }

    private fun startTypedForeground(audioMode: AudioMode) {
        val notification = buildNotification()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            var serviceTypes = ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION
            if (
                audioMode == AudioMode.Microphone &&
                Build.VERSION.SDK_INT >= Build.VERSION_CODES.R
            ) {
                serviceTypes = serviceTypes or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
            }
            startForeground(NOTIFICATION_ID, notification, serviceTypes)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun buildNotification(): Notification {
        val openIntent = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val stopIntent = PendingIntent.getService(
            this,
            1,
            Intent(this, RecordingService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        return Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_recording_notification)
            .setContentTitle(getString(R.string.recording_notification_title))
            .setContentText("点击返回 ShiPing，或使用下方操作停止录制")
            .setContentIntent(openIntent)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .addAction(
                android.R.drawable.ic_media_pause,
                getString(R.string.recording_notification_stop),
                stopIntent,
            )
            .build()
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            getString(R.string.recording_channel_name),
            NotificationManager.IMPORTANCE_LOW,
        )
        channel.description = "显示正在进行的屏幕录制"
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    override fun onRecording(elapsedMs: Long) {
        RecordingStateStore.recording(elapsedMs)
    }

    override fun onFinalizing(elapsedMs: Long) {
        RecordingStateStore.finalizing(elapsedMs)
    }

    override fun onCompleted(elapsedMs: Long, outputUri: String) {
        recorder = null
        RecordingStateStore.completed(elapsedMs, outputUri)
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onFailed(elapsedMs: Long, message: String) {
        recorder = null
        RecordingStateStore.failed(message, elapsedMs)
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    companion object {
        private const val ACTION_START = "com.dripai.shiping.action.START_RECORDING"
        private const val ACTION_STOP = "com.dripai.shiping.action.STOP_RECORDING"
        private const val EXTRA_RESULT_CODE = "result_code"
        private const val EXTRA_PROJECTION_DATA = "projection_data"
        private const val EXTRA_QUALITY = "quality"
        private const val EXTRA_FRAME_RATE = "frame_rate"
        private const val EXTRA_AUDIO_MODE = "audio_mode"
        private const val CHANNEL_ID = "shiping_recording"
        private const val NOTIFICATION_ID = 1108

        fun start(
            context: Context,
            resultCode: Int,
            projectionData: Intent,
            config: RecordingConfig,
        ) {
            val intent = Intent(context, RecordingService::class.java)
                .setAction(ACTION_START)
                .putExtra(EXTRA_RESULT_CODE, resultCode)
                .putExtra(EXTRA_PROJECTION_DATA, projectionData)
                .putExtra(EXTRA_QUALITY, config.quality.name)
                .putExtra(EXTRA_FRAME_RATE, config.frameRate)
                .putExtra(EXTRA_AUDIO_MODE, config.audioMode.name)
            ContextCompat.startForegroundService(context, intent)
        }

        fun stop(context: Context) {
            val intent = Intent(context, RecordingService::class.java).setAction(ACTION_STOP)
            context.startService(intent)
        }

        private fun Intent.toRecordingConfig(): RecordingConfig {
            return RecordingConfig(
                quality = enumValueOrDefault(
                    getStringExtra(EXTRA_QUALITY),
                    VideoQuality.FullHd,
                ),
                frameRate = getIntExtra(EXTRA_FRAME_RATE, 30).coerceIn(24, 60),
                audioMode = enumValueOrDefault(
                    getStringExtra(EXTRA_AUDIO_MODE),
                    AudioMode.System,
                ),
            )
        }

        private inline fun <reified T : Enum<T>> enumValueOrDefault(
            value: String?,
            default: T,
        ): T = runCatching { enumValueOf<T>(value.orEmpty()) }.getOrDefault(default)
    }
}

private fun Intent.parcelableIntentExtra(name: String): Intent? {
    return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        getParcelableExtra(name, Intent::class.java)
    } else {
        @Suppress("DEPRECATION")
        getParcelableExtra(name)
    }
}

private fun Throwable.readableMessage(): String =
    message?.takeIf(String::isNotBlank) ?: javaClass.simpleName
