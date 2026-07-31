package com.dripai.shiping

object RustBridge {
    init {
        System.loadLibrary("shiping_android")
    }

    @JvmStatic
    private external fun nativeUpdateState(state: Int, elapsedMs: Long)

    @JvmStatic
    private external fun nativeCurrentState(): Int

    @JvmStatic
    private external fun nativeElapsedMs(): Long

    fun updateState(state: Int, elapsedMs: Long) {
        nativeUpdateState(state, elapsedMs)
    }

    fun currentState(): Int = nativeCurrentState()

    fun elapsedMs(): Long = nativeElapsedMs()
}
