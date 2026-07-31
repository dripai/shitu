package com.dripai.shiping.recording

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

object RecordingStateStore {
    private val mutableState = MutableStateFlow(RecordingUiState())
    val state: StateFlow<RecordingUiState> = mutableState.asStateFlow()

    fun authorizing(message: String = "正在等待系统录屏授权") {
        update(
            phase = RecordingPhase.Authorizing,
            elapsedMs = 0,
            message = message,
        )
    }

    fun recording(elapsedMs: Long) {
        update(
            phase = RecordingPhase.Recording,
            elapsedMs = elapsedMs,
            message = "正在录制",
        )
    }

    fun finalizing(elapsedMs: Long) {
        update(
            phase = RecordingPhase.Finalizing,
            elapsedMs = elapsedMs,
            message = "正在保存 MP4",
        )
    }

    fun completed(elapsedMs: Long, outputUri: String) {
        mutableState.value = RecordingUiState(
            phase = RecordingPhase.Completed,
            elapsedMs = elapsedMs,
            message = "录制已保存到系统视频目录",
            outputUri = outputUri,
        )
    }

    fun failed(message: String, elapsedMs: Long = mutableState.value.elapsedMs) {
        mutableState.value = RecordingUiState(
            phase = RecordingPhase.Failed,
            elapsedMs = elapsedMs,
            message = message,
            outputUri = null,
        )
    }

    fun idle(message: String = "准备开始录制") {
        mutableState.value = RecordingUiState(message = message)
    }

    private fun update(
        phase: RecordingPhase,
        elapsedMs: Long,
        message: String,
    ) {
        mutableState.value = mutableState.value.copy(
            phase = phase,
            elapsedMs = elapsedMs,
            message = message,
            outputUri = null,
        )
    }
}
