package com.dripai.shiping.recording

import android.content.Context
import android.graphics.Color
import android.graphics.PixelFormat
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.WindowManager
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import java.util.Locale
import kotlin.math.abs

class RecordingOverlayController(
    context: Context,
) {
    private val appContext = context.applicationContext
    private val windowManager = appContext.getSystemService(WindowManager::class.java)
    private val mainHandler = Handler(Looper.getMainLooper())
    private var overlayView: View? = null
    private var elapsedView: TextView? = null

    fun show(onStop: () -> Unit) {
        check(Looper.myLooper() == Looper.getMainLooper()) {
            "Recording overlay must be created on the main thread"
        }
        check(Settings.canDrawOverlays(appContext)) {
            "没有显示录制悬浮状态的系统权限"
        }
        if (overlayView != null) {
            return
        }

        val view = createOverlayView(onStop)
        val params = WindowManager.LayoutParams(
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
                WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL or
                WindowManager.LayoutParams.FLAG_SECURE,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            x = (appContext.resources.displayMetrics.widthPixels - dp(148)).coerceAtLeast(dp(12))
            y = dp(132)
        }

        view.setOnTouchListener(OverlayDragListener(params))
        windowManager.addView(view, params)
        overlayView = view
    }

    fun updateElapsed(elapsedMs: Long) {
        mainHandler.post {
            elapsedView?.text = formatElapsed(elapsedMs)
        }
    }

    fun hide() {
        if (Looper.myLooper() != Looper.getMainLooper()) {
            mainHandler.post(::hide)
            return
        }
        overlayView?.let { view ->
            runCatching { windowManager.removeView(view) }
        }
        overlayView = null
        elapsedView = null
    }

    private fun createOverlayView(onStop: () -> Unit): View {
        val root = LinearLayout(appContext).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            minimumWidth = dp(132)
            minimumHeight = dp(52)
            setPadding(dp(6), dp(6), dp(14), dp(6))
            background = roundedBackground(0xE6292D36.toInt(), dp(28).toFloat())
            contentDescription = "ShiPing 正在录制"
        }

        val stopButton = FrameLayout(appContext).apply {
            isClickable = true
            isFocusable = true
            contentDescription = "停止并保存录制"
            background = roundedBackground(Color.WHITE, dp(22).toFloat())
            setOnClickListener { onStop() }
        }
        val stopMark = View(appContext).apply {
            background = roundedBackground(0xFFEF4444.toInt(), dp(2).toFloat())
        }
        stopButton.addView(
            stopMark,
            FrameLayout.LayoutParams(dp(13), dp(13), Gravity.CENTER),
        )
        root.addView(stopButton, LinearLayout.LayoutParams(dp(40), dp(40)))

        elapsedView = TextView(appContext).apply {
            text = "00:00"
            setTextColor(Color.WHITE)
            textSize = 17f
            typeface = Typeface.MONOSPACE
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(10), 0, 0, 0)
        }
        root.addView(
            elapsedView,
            LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 1f),
        )
        return root
    }

    private fun roundedBackground(color: Int, radius: Float): GradientDrawable =
        GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            setColor(color)
            cornerRadius = radius
        }

    private fun dp(value: Int): Int =
        (value * appContext.resources.displayMetrics.density).toInt()

    private inner class OverlayDragListener(
        private val params: WindowManager.LayoutParams,
    ) : View.OnTouchListener {
        private var startX = 0
        private var startY = 0
        private var touchX = 0f
        private var touchY = 0f
        private var dragged = false

        override fun onTouch(view: View, event: MotionEvent): Boolean {
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    startX = params.x
                    startY = params.y
                    touchX = event.rawX
                    touchY = event.rawY
                    dragged = false
                    return true
                }
                MotionEvent.ACTION_MOVE -> {
                    val deltaX = (event.rawX - touchX).toInt()
                    val deltaY = (event.rawY - touchY).toInt()
                    dragged = dragged || abs(deltaX) > dp(3) || abs(deltaY) > dp(3)
                    params.x = (startX + deltaX).coerceAtLeast(0)
                    params.y = (startY + deltaY).coerceAtLeast(0)
                    overlayView?.let { windowManager.updateViewLayout(it, params) }
                    return true
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    if (!dragged) {
                        view.performClick()
                    }
                    return true
                }
            }
            return false
        }
    }

    companion object {
        private fun formatElapsed(elapsedMs: Long): String {
            val totalSeconds = elapsedMs / 1_000
            val minutes = totalSeconds / 60
            val seconds = totalSeconds % 60
            return String.format(Locale.ROOT, "%02d:%02d", minutes, seconds)
        }
    }
}
