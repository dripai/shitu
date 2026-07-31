package com.dripai.shiping.recording

enum class RecordingPhase {
    Idle,
    Authorizing,
    Recording,
    Finalizing,
    Completed,
    Failed,
}

enum class VideoQuality(val label: String, val longEdge: Int?) {
    Original("原始分辨率", null),
    FullHd("1080p", 1920),
    Hd("720p", 1280),
}

enum class AudioMode(val label: String) {
    None("无声音"),
    System("系统声音"),
    Microphone("麦克风"),
}

data class RecordingConfig(
    val quality: VideoQuality = VideoQuality.FullHd,
    val frameRate: Int = 30,
    val audioMode: AudioMode = AudioMode.System,
)

data class RecordingUiState(
    val phase: RecordingPhase = RecordingPhase.Idle,
    val elapsedMs: Long = 0,
    val message: String = "准备开始录制",
    val outputUri: String? = null,
) {
    val isActive: Boolean
        get() = phase == RecordingPhase.Authorizing ||
            phase == RecordingPhase.Recording ||
            phase == RecordingPhase.Finalizing
}
