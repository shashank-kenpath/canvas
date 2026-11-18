package com.canvasstudio.rust

import android.content.Context
import android.view.MotionEvent
import android.view.SurfaceView

/** Only a window and input pipe. All visible controls are drawn by Rust. */
class BoardSurface(context: Context) : SurfaceView(context) {
    var handle = 0L
    private var samples = FloatArray(STRIDE * 64)
    private val followed = HashSet<Int>()
    var scale = 1f
        private set
    var maxSide = MAX_SIDE
        set(value) { field = value; fit(width, height) }

    private fun fit(w: Int, h: Int) {
        if (w <= 0 || h <= 0) return
        scale = if (maxSide > 0 && maxOf(w, h) > maxSide) maxSide.toFloat() / maxOf(w, h) else 1f
        if (scale < 1f) holder.setFixedSize(Math.round(w * scale), Math.round(h * scale)) else holder.setSizeFromLayout()
    }
    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) { super.onSizeChanged(w, h, oldw, oldh); fit(w, h) }
    private fun put(k: Int, e: MotionEvent, p: Int, x: Float, y: Float, time: Long, major: Float) {
        val o = k * STRIDE
        if (o + STRIDE > samples.size) samples = samples.copyOf(maxOf(samples.size * 2, o + STRIDE))
        samples[o] = e.getPointerId(p).toFloat()
        samples[o + 1] = x * scale
        samples[o + 2] = y * scale
        samples[o + 3] = (time - e.downTime).toFloat()
        samples[o + 4] = major * scale
        samples[o + 5] = when (e.getToolType(p)) { MotionEvent.TOOL_TYPE_ERASER -> 2f; MotionEvent.TOOL_TYPE_STYLUS -> 1f; else -> 0f }
    }
    private fun one(action: Int, e: MotionEvent, p: Int) {
        put(0, e, p, e.getX(p), e.getY(p), e.eventTime, e.getTouchMajor(p))
        RustApp.touch(handle, action, e.downTime, samples, 1)
    }
    override fun onTouchEvent(e: MotionEvent): Boolean {
        if (handle == 0L) return false
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                if (e.actionMasked == MotionEvent.ACTION_DOWN) { requestUnbufferedDispatch(e); followed.clear() }
                if (followed.size < MAX_POINTERS) { followed.add(e.getPointerId(e.actionIndex)); one(0, e, e.actionIndex) }
            }
            MotionEvent.ACTION_MOVE -> {
                var n = 0
                for (h in 0 until e.historySize) for (p in 0 until e.pointerCount) {
                    if (e.getPointerId(p) in followed) put(n++, e, p, e.getHistoricalX(p, h), e.getHistoricalY(p, h), e.getHistoricalEventTime(h), e.getHistoricalTouchMajor(p, h))
                }
                for (p in 0 until e.pointerCount) if (e.getPointerId(p) in followed) put(n++, e, p, e.getX(p), e.getY(p), e.eventTime, e.getTouchMajor(p))
                if (n > 0) RustApp.touch(handle, 1, e.downTime, samples, n)
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> if (followed.remove(e.getPointerId(e.actionIndex))) one(2, e, e.actionIndex)
            MotionEvent.ACTION_CANCEL -> {
                var n = 0
                for (p in 0 until e.pointerCount) if (e.getPointerId(p) in followed) put(n++, e, p, e.getX(p), e.getY(p), e.eventTime, e.getTouchMajor(p))
                followed.clear()
                RustApp.touch(handle, 3, e.downTime, samples, n)
            }
        }
        return true
    }
    companion object { const val STRIDE = 6; const val MAX_SIDE = 1920; const val MAX_POINTERS = 5 }
}
