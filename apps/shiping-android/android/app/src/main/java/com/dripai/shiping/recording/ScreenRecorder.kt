package com.dripai.shiping.recording

import android.content.ContentValues
import android.content.Context
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioPlaybackCaptureConfiguration
import android.media.AudioRecord
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.media.MediaMuxer
import android.media.MediaRecorder
import android.media.MediaScannerConnection
import android.media.projection.MediaProjection
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.MediaStore
import android.view.Surface
import android.view.WindowManager
import java.io.File
import java.nio.ByteBuffer
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.max
import kotlin.math.roundToInt

class ScreenRecorder(
    private val context: Context,
    private val projection: MediaProjection,
    private val config: RecordingConfig,
    private val callbacks: Callbacks,
) {
    interface Callbacks {
        fun onRecording(elapsedMs: Long)
        fun onFinalizing(elapsedMs: Long)
        fun onCompleted(elapsedMs: Long, outputUri: String)
        fun onFailed(elapsedMs: Long, message: String)
    }

    private val stopRequested = AtomicBoolean(false)
    private var worker: Thread? = null

    fun start() {
        check(worker == null) { "ScreenRecorder can only be started once" }
        worker = Thread(::record, "shiping-android-recorder").also(Thread::start)
    }

    fun stop() {
        stopRequested.set(true)
    }

    private fun record() {
        var output: RecordingOutput? = null
        var videoEncoder: MediaCodec? = null
        var audioEncoder: MediaCodec? = null
        var audioRecord: AudioRecord? = null
        var inputSurface: Surface? = null
        var virtualDisplay: VirtualDisplay? = null
        var muxerStarted = false
        val startRealtime = SystemClock.elapsedRealtime()

        val projectionCallback = object : MediaProjection.Callback() {
            override fun onStop() {
                stopRequested.set(true)
            }
        }

        try {
            projection.registerCallback(projectionCallback, Handler(Looper.getMainLooper()))
            output = RecordingOutput.create(context)

            val size = resolveVideoSize()
            val videoFormat = MediaFormat.createVideoFormat(
                MediaFormat.MIMETYPE_VIDEO_AVC,
                size.width,
                size.height,
            ).apply {
                setInteger(
                    MediaFormat.KEY_COLOR_FORMAT,
                    MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface,
                )
                setInteger(MediaFormat.KEY_BIT_RATE, videoBitRate(size))
                setInteger(MediaFormat.KEY_FRAME_RATE, config.frameRate)
                setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 2)
                setInteger(
                    MediaFormat.KEY_BITRATE_MODE,
                    MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_VBR,
                )
            }

            videoEncoder = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC).apply {
                configure(videoFormat, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
                inputSurface = createInputSurface()
                start()
            }

            val audioPipeline = createAudioPipeline()
            audioEncoder = audioPipeline?.encoder
            audioRecord = audioPipeline?.record
            audioEncoder?.start()
            audioRecord?.startRecording()

            virtualDisplay = projection.createVirtualDisplay(
                "ShiPing Screen Recording",
                size.width,
                size.height,
                context.resources.displayMetrics.densityDpi,
                DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR,
                requireNotNull(inputSurface),
                null,
                null,
            )

            output.muxer.setOrientationHint(currentRotationDegrees())
            callbacks.onRecording(0)

            val drainResult = drainEncoders(
                videoEncoder = videoEncoder,
                audioEncoder = audioEncoder,
                audioRecord = audioRecord,
                muxer = output.muxer,
                startRealtime = startRealtime,
            )
            muxerStarted = drainResult.muxerStarted

            val elapsedMs = SystemClock.elapsedRealtime() - startRealtime
            callbacks.onFinalizing(elapsedMs)
            if (!muxerStarted) {
                error("编码器没有生成可写入的媒体轨道")
            }

            output.muxer.stop()
            output.publish()
            callbacks.onCompleted(elapsedMs, output.uri.toString())
        } catch (error: Exception) {
            val elapsedMs = (SystemClock.elapsedRealtime() - startRealtime).coerceAtLeast(0)
            output?.discard()
            callbacks.onFailed(
                elapsedMs,
                "录制失败：${error.message?.takeIf(String::isNotBlank) ?: error.javaClass.simpleName}",
            )
        } finally {
            runCatching { audioRecord?.stop() }
            audioRecord?.release()
            runCatching { audioEncoder?.stop() }
            audioEncoder?.release()
            virtualDisplay?.release()
            inputSurface?.release()
            runCatching { videoEncoder?.stop() }
            videoEncoder?.release()
            if (!muxerStarted) {
                output?.releaseWithoutStopping()
            } else {
                output?.releaseAfterStop()
            }
            runCatching { projection.unregisterCallback(projectionCallback) }
            runCatching { projection.stop() }
        }
    }

    private fun createAudioPipeline(): AudioPipeline? {
        if (config.audioMode == AudioMode.None) {
            return null
        }
        if (config.audioMode == AudioMode.System && Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            error("系统声音录制需要 Android 10 或更高版本")
        }

        val channelMask = if (config.audioMode == AudioMode.Microphone) {
            AudioFormat.CHANNEL_IN_MONO
        } else {
            AudioFormat.CHANNEL_IN_STEREO
        }
        val channelCount = if (channelMask == AudioFormat.CHANNEL_IN_MONO) 1 else 2
        val audioFormat = AudioFormat.Builder()
            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
            .setSampleRate(AUDIO_SAMPLE_RATE)
            .setChannelMask(channelMask)
            .build()
        val minimumBuffer = AudioRecord.getMinBufferSize(
            AUDIO_SAMPLE_RATE,
            channelMask,
            AudioFormat.ENCODING_PCM_16BIT,
        )
        check(minimumBuffer > 0) { "设备不支持所选音频格式" }
        val bufferSize = max(minimumBuffer * 2, AUDIO_SAMPLE_RATE / 5 * channelCount * 2)

        val recordBuilder = AudioRecord.Builder()
            .setAudioFormat(audioFormat)
            .setBufferSizeInBytes(bufferSize)

        if (config.audioMode == AudioMode.System) {
            val captureConfig = AudioPlaybackCaptureConfiguration.Builder(projection)
                .addMatchingUsage(AudioAttributes.USAGE_MEDIA)
                .addMatchingUsage(AudioAttributes.USAGE_GAME)
                .addMatchingUsage(AudioAttributes.USAGE_UNKNOWN)
                .build()
            recordBuilder.setAudioPlaybackCaptureConfig(captureConfig)
        } else {
            recordBuilder.setAudioSource(MediaRecorder.AudioSource.MIC)
        }

        val record = recordBuilder.build()
        check(record.state == AudioRecord.STATE_INITIALIZED) {
            "无法初始化${config.audioMode.label}采集"
        }

        val encoderFormat = MediaFormat.createAudioFormat(
            MediaFormat.MIMETYPE_AUDIO_AAC,
            AUDIO_SAMPLE_RATE,
            channelCount,
        ).apply {
            setInteger(
                MediaFormat.KEY_AAC_PROFILE,
                MediaCodecInfo.CodecProfileLevel.AACObjectLC,
            )
            setInteger(MediaFormat.KEY_BIT_RATE, if (channelCount == 1) 96_000 else 128_000)
            setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, bufferSize)
        }
        val encoder = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_AUDIO_AAC).apply {
            configure(encoderFormat, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
        }
        return AudioPipeline(encoder, record)
    }

    private fun drainEncoders(
        videoEncoder: MediaCodec,
        audioEncoder: MediaCodec?,
        audioRecord: AudioRecord?,
        muxer: MediaMuxer,
        startRealtime: Long,
    ): DrainResult {
        val videoInfo = MediaCodec.BufferInfo()
        val audioInfo = MediaCodec.BufferInfo()
        val pendingSamples = mutableListOf<PendingSample>()
        val expectsAudio = audioEncoder != null
        var videoTrack = -1
        var audioTrack = -1
        var muxerStarted = false
        var videoEosRequested = false
        var audioEosQueued = !expectsAudio
        var videoDone = false
        var audioDone = !expectsAudio
        var lastProgressAt = 0L
        var stopDeadline = Long.MAX_VALUE

        fun startMuxerIfReady() {
            if (muxerStarted || videoTrack < 0 || (expectsAudio && audioTrack < 0)) {
                return
            }
            muxer.start()
            muxerStarted = true
            pendingSamples.forEach { sample ->
                val track = if (sample.video) videoTrack else audioTrack
                val info = MediaCodec.BufferInfo().apply {
                    set(0, sample.data.size, sample.presentationTimeUs, sample.flags)
                }
                muxer.writeSampleData(track, ByteBuffer.wrap(sample.data), info)
            }
            pendingSamples.clear()
        }

        fun writeSample(
            video: Boolean,
            buffer: ByteBuffer,
            info: MediaCodec.BufferInfo,
        ) {
            if (info.size <= 0 || info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
                return
            }
            buffer.position(info.offset)
            buffer.limit(info.offset + info.size)

            if (muxerStarted) {
                muxer.writeSampleData(if (video) videoTrack else audioTrack, buffer, info)
            } else {
                val bytes = ByteArray(info.size)
                buffer.get(bytes)
                pendingSamples += PendingSample(
                    video = video,
                    data = bytes,
                    presentationTimeUs = info.presentationTimeUs,
                    flags = info.flags,
                )
            }
        }

        fun drainVideo(): Boolean {
            var progressed = false
            while (true) {
                when (val index = videoEncoder.dequeueOutputBuffer(videoInfo, 0)) {
                    MediaCodec.INFO_TRY_AGAIN_LATER -> return progressed
                    MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                        check(videoTrack < 0) { "视频编码格式重复变化" }
                        videoTrack = muxer.addTrack(videoEncoder.outputFormat)
                        startMuxerIfReady()
                        progressed = true
                    }
                    else -> if (index >= 0) {
                        val buffer = requireNotNull(videoEncoder.getOutputBuffer(index))
                        writeSample(true, buffer, videoInfo)
                        if (videoInfo.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) {
                            videoDone = true
                        }
                        videoEncoder.releaseOutputBuffer(index, false)
                        progressed = true
                    }
                }
            }
        }

        fun drainAudio(): Boolean {
            val encoder = audioEncoder ?: return false
            var progressed = false
            while (true) {
                when (val index = encoder.dequeueOutputBuffer(audioInfo, 0)) {
                    MediaCodec.INFO_TRY_AGAIN_LATER -> return progressed
                    MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                        check(audioTrack < 0) { "音频编码格式重复变化" }
                        audioTrack = muxer.addTrack(encoder.outputFormat)
                        startMuxerIfReady()
                        progressed = true
                    }
                    else -> if (index >= 0) {
                        val buffer = requireNotNull(encoder.getOutputBuffer(index))
                        writeSample(false, buffer, audioInfo)
                        if (audioInfo.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) {
                            audioDone = true
                        }
                        encoder.releaseOutputBuffer(index, false)
                        progressed = true
                    }
                }
            }
        }

        fun feedAudio(): Boolean {
            val encoder = audioEncoder ?: return false
            val record = audioRecord ?: return false
            val index = encoder.dequeueInputBuffer(0)
            if (index < 0) {
                return false
            }
            val buffer = requireNotNull(encoder.getInputBuffer(index))
            buffer.clear()
            val read = record.read(buffer, buffer.capacity(), AudioRecord.READ_NON_BLOCKING)
            if (read < 0) {
                error("音频采集失败，错误码 $read")
            }
            encoder.queueInputBuffer(
                index,
                0,
                read,
                System.nanoTime() / 1_000,
                0,
            )
            return read > 0
        }

        fun queueAudioEos(): Boolean {
            val encoder = audioEncoder ?: return true
            val index = encoder.dequeueInputBuffer(0)
            if (index < 0) {
                return false
            }
            encoder.queueInputBuffer(
                index,
                0,
                0,
                System.nanoTime() / 1_000,
                MediaCodec.BUFFER_FLAG_END_OF_STREAM,
            )
            return true
        }

        while (!videoDone || !audioDone) {
            var progressed = false
            val now = SystemClock.elapsedRealtime()

            if (!stopRequested.get()) {
                progressed = feedAudio() || progressed
                if (now - lastProgressAt >= PROGRESS_INTERVAL_MS) {
                    callbacks.onRecording(now - startRealtime)
                    lastProgressAt = now
                }
            } else {
                if (!videoEosRequested) {
                    videoEncoder.signalEndOfInputStream()
                    videoEosRequested = true
                    runCatching { audioRecord?.stop() }
                    stopDeadline = now + FINALIZE_TIMEOUT_MS
                }
                if (!audioEosQueued) {
                    audioEosQueued = queueAudioEos()
                }
                if (now >= stopDeadline) {
                    error("编码器在停止时超时")
                }
            }

            progressed = drainVideo() || progressed
            progressed = drainAudio() || progressed
            if (!progressed) {
                Thread.sleep(4)
            }
        }

        check(pendingSamples.isEmpty()) { "媒体轨道未就绪，无法写入编码数据" }
        return DrainResult(muxerStarted)
    }

    private fun resolveVideoSize(): VideoSize {
        val metrics = context.resources.displayMetrics
        val sourceWidth = metrics.widthPixels.coerceAtLeast(2)
        val sourceHeight = metrics.heightPixels.coerceAtLeast(2)
        val targetLongEdge = config.quality.longEdge
        if (targetLongEdge == null) {
            return VideoSize(sourceWidth.even(), sourceHeight.even())
        }

        val sourceLongEdge = max(sourceWidth, sourceHeight)
        val scale = (targetLongEdge.toFloat() / sourceLongEdge).coerceAtMost(1f)
        return VideoSize(
            width = (sourceWidth * scale).roundToInt().coerceAtLeast(2).even(),
            height = (sourceHeight * scale).roundToInt().coerceAtLeast(2).even(),
        )
    }

    private fun videoBitRate(size: VideoSize): Int {
        return (size.width.toLong() * size.height * config.frameRate / 8)
            .coerceIn(2_000_000, 24_000_000)
            .toInt()
    }

    @Suppress("DEPRECATION")
    private fun currentRotationDegrees(): Int {
        val rotation = (context.getSystemService(Context.WINDOW_SERVICE) as WindowManager)
            .defaultDisplay
            .rotation
        return when (rotation) {
            Surface.ROTATION_90 -> 90
            Surface.ROTATION_180 -> 180
            Surface.ROTATION_270 -> 270
            else -> 0
        }
    }

    private data class VideoSize(val width: Int, val height: Int)
    private data class AudioPipeline(val encoder: MediaCodec, val record: AudioRecord)
    private data class DrainResult(val muxerStarted: Boolean)
    private data class PendingSample(
        val video: Boolean,
        val data: ByteArray,
        val presentationTimeUs: Long,
        val flags: Int,
    )

    companion object {
        private const val AUDIO_SAMPLE_RATE = 48_000
        private const val PROGRESS_INTERVAL_MS = 250L
        private const val FINALIZE_TIMEOUT_MS = 8_000L
    }
}

private class RecordingOutput private constructor(
    private val context: Context,
    val uri: Uri,
    val muxer: MediaMuxer,
    private val pendingInMediaStore: Boolean,
    private val scanPath: String?,
) {
    private var released = false

    fun publish() {
        if (pendingInMediaStore) {
            val values = ContentValues().apply {
                put(MediaStore.Video.Media.IS_PENDING, 0)
            }
            context.contentResolver.update(uri, values, null, null)
        } else if (scanPath != null) {
            MediaScannerConnection.scanFile(
                context,
                arrayOf(scanPath),
                arrayOf("video/mp4"),
                null,
            )
        }
    }

    fun discard() {
        if (!released) {
            runCatching { muxer.release() }
            released = true
        }
        if (pendingInMediaStore) {
            context.contentResolver.delete(uri, null, null)
        } else {
            scanPath?.let(::File)?.delete()
        }
    }

    fun releaseWithoutStopping() {
        if (!released) {
            muxer.release()
            released = true
        }
    }

    fun releaseAfterStop() {
        if (!released) {
            muxer.release()
            released = true
        }
    }

    companion object {
        fun create(context: Context): RecordingOutput {
            val timestamp = SimpleDateFormat("yyyyMMdd_HHmmss", Locale.ROOT).format(Date())
            val fileName = "ShiPing_$timestamp.mp4"

            return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                val values = ContentValues().apply {
                    put(MediaStore.Video.Media.DISPLAY_NAME, fileName)
                    put(MediaStore.Video.Media.MIME_TYPE, "video/mp4")
                    put(
                        MediaStore.Video.Media.RELATIVE_PATH,
                        "${Environment.DIRECTORY_MOVIES}/ShiPing",
                    )
                    put(MediaStore.Video.Media.IS_PENDING, 1)
                }
                val collection = MediaStore.Video.Media.getContentUri(
                    MediaStore.VOLUME_EXTERNAL_PRIMARY,
                )
                val uri = checkNotNull(context.contentResolver.insert(collection, values)) {
                    "无法在系统视频目录创建文件"
                }
                try {
                    val descriptor = checkNotNull(
                        context.contentResolver.openFileDescriptor(uri, "w"),
                    ) {
                        "无法打开录制输出文件"
                    }
                    val muxer = descriptor.use {
                        MediaMuxer(
                            it.fileDescriptor,
                            MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4,
                        )
                    }
                    RecordingOutput(context, uri, muxer, true, null)
                } catch (error: Exception) {
                    context.contentResolver.delete(uri, null, null)
                    throw error
                }
            } else {
                val directory = File(
                    context.getExternalFilesDir(Environment.DIRECTORY_MOVIES),
                    "ShiPing",
                )
                check(directory.exists() || directory.mkdirs()) {
                    "无法创建录制输出目录"
                }
                val file = File(directory, fileName)
                RecordingOutput(
                    context = context,
                    uri = Uri.fromFile(file),
                    muxer = MediaMuxer(
                        file.absolutePath,
                        MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4,
                    ),
                    pendingInMediaStore = false,
                    scanPath = file.absolutePath,
                )
            }
        }
    }
}

private fun Int.even(): Int = this and 1.inv()
