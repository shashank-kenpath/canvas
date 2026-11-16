package com.canvasstudio.rust

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.drawable.GradientDrawable
import android.view.MotionEvent
import android.view.SurfaceView
import android.view.View

const val INK = 0xff1d1d1d.toInt()
const val ACCENT = 0xff2f6fed.toInt()

fun Context.dp(v: Float): Int = Math.round(v * resources.displayMetrics.density)

fun rounded(color: Int, radius: Float, stroke: Int = 0, strokeWidth: Int = 0) = GradientDrawable().apply {
    setColor(color)
    cornerRadius = radius
    if (strokeWidth > 0) setStroke(strokeWidth, stroke)
}

enum class Icon { SELECT, HAND, PEN, RECTANGLE, ELLIPSE, ARROW, ERASER, UNDO, REDO, STYLE, FONT, DELETE, MORE, BACK }

/** A square toolbar button that draws its own icon. */
class IconButton(context: Context, private val icon: Icon, private val description: String) : View(context) {
    private val line = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
    }
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG)
    private val path = Path()

    /** Colour shown inside the style button. */
    var swatch: Int = INK
        set(value) {
            field = value
            invalidate()
        }

    var active: Boolean = false
        set(value) {
            if (field != value) {
                field = value
                invalidate()
            }
        }

    init {
        contentDescription = description
        isClickable = true
        isFocusable = true
    }

    override fun setEnabled(enabled: Boolean) {
        if (enabled != isEnabled) {
            super.setEnabled(enabled)
            invalidate()
        }
    }

    override fun onMeasure(widthSpec: Int, heightSpec: Int) {
        val s = context.dp(46f)
        setMeasuredDimension(s, s)
    }

    override fun drawableStateChanged() {
        super.drawableStateChanged()
        invalidate()
    }

    override fun onDraw(c: Canvas) {
        val w = width.toFloat()
        val pad = w * 0.09f
        if (active || isPressed || isFocused) {
            fill.color = if (active) ACCENT else 0x14000000
            c.drawRoundRect(RectF(pad, pad, w - pad, w - pad), w * 0.2f, w * 0.2f, fill)
        }
        val color = if (active) 0xffffffff.toInt() else INK
        line.color = color
        line.alpha = if (isEnabled) 255 else 70
        line.strokeWidth = w * 0.05f
        fill.color = color
        fill.alpha = line.alpha
        // Icons are drawn on a 24 x 24 grid.
        c.save()
        c.translate(w * 0.25f, w * 0.25f)
        c.scale(w / 48f, w / 48f)
        line.strokeWidth = 1.9f
        path.reset()
        when (icon) {
            Icon.SELECT -> {
                path.moveTo(5f, 3f); path.lineTo(19f, 12f); path.lineTo(12.5f, 13.5f); path.lineTo(9.5f, 20f); path.close()
                c.drawPath(path, line)
            }
            Icon.HAND -> {
                path.moveTo(8f, 13f); path.lineTo(8f, 5.5f)
                path.moveTo(11.5f, 12f); path.lineTo(11.5f, 3.5f)
                path.moveTo(15f, 12f); path.lineTo(15f, 5f)
                path.moveTo(18.5f, 13f); path.lineTo(18.5f, 8f)
                path.moveTo(8f, 13f); path.lineTo(5f, 11f)
                path.moveTo(5f, 11f); path.cubicTo(5f, 17f, 8f, 21f, 12.5f, 21f)
                path.cubicTo(16.5f, 21f, 18.5f, 18f, 18.5f, 13f)
                c.drawPath(path, line)
            }
            Icon.PEN -> {
                path.moveTo(4f, 20f); path.lineTo(5.2f, 15.2f); path.lineTo(16.5f, 3.9f); path.lineTo(20.1f, 7.5f)
                path.lineTo(8.8f, 18.8f); path.close()
                path.moveTo(14.2f, 6.2f); path.lineTo(17.8f, 9.8f)
                c.drawPath(path, line)
            }
            Icon.RECTANGLE -> c.drawRoundRect(RectF(4f, 6f, 20f, 18f), 1.5f, 1.5f, line)
            Icon.ELLIPSE -> c.drawOval(RectF(3.5f, 5.5f, 20.5f, 18.5f), line)
            Icon.ARROW -> {
                path.moveTo(5f, 19f); path.lineTo(19f, 5f)
                path.moveTo(11f, 5f); path.lineTo(19f, 5f); path.lineTo(19f, 13f)
                c.drawPath(path, line)
            }
            Icon.ERASER -> {
                path.moveTo(9f, 20f); path.lineTo(4f, 15f); path.lineTo(14f, 5f); path.lineTo(20f, 11f); path.lineTo(11f, 20f); path.close()
                path.moveTo(8.5f, 10.5f); path.lineTo(14.5f, 16.5f)
                path.moveTo(11f, 20f); path.lineTo(20f, 20f)
                c.drawPath(path, line)
            }
            Icon.UNDO, Icon.REDO -> {
                if (icon == Icon.REDO) c.scale(-1f, 1f, 12f, 12f)
                path.moveTo(9f, 5f); path.lineTo(4.5f, 9.5f); path.lineTo(9f, 14f)
                path.moveTo(4.5f, 9.5f); path.lineTo(14f, 9.5f)
                path.cubicTo(21.5f, 9.5f, 21.5f, 19f, 14f, 19f); path.lineTo(10f, 19f)
                c.drawPath(path, line)
            }
            Icon.STYLE -> {
                fill.color = swatch
                fill.alpha = 255
                c.drawCircle(12f, 12f, 7.5f, fill)
                line.color = 0x55000000
                line.strokeWidth = 1.2f
                c.drawCircle(12f, 12f, 7.5f, line)
            }
            Icon.FONT -> {
                path.moveTo(5f, 19f); path.lineTo(11f, 5f); path.lineTo(13f, 5f); path.lineTo(19f, 19f)
                path.moveTo(7.6f, 14f); path.lineTo(16.4f, 14f)
                c.drawPath(path, line)
            }
            Icon.DELETE -> {
                path.moveTo(4.5f, 7f); path.lineTo(19.5f, 7f)
                path.moveTo(9.5f, 7f); path.lineTo(9.5f, 4.5f); path.lineTo(14.5f, 4.5f); path.lineTo(14.5f, 7f)
                path.moveTo(6.5f, 7f); path.lineTo(7.5f, 20f); path.lineTo(16.5f, 20f); path.lineTo(17.5f, 7f)
                path.moveTo(10.3f, 10.5f); path.lineTo(10.6f, 16.5f)
                path.moveTo(13.7f, 10.5f); path.lineTo(13.4f, 16.5f)
                c.drawPath(path, line)
            }
            Icon.MORE -> {
                c.drawCircle(5.5f, 12f, 1.7f, fill)
                c.drawCircle(12f, 12f, 1.7f, fill)
                c.drawCircle(18.5f, 12f, 1.7f, fill)
            }
            Icon.BACK -> {
                path.moveTo(11f, 5f); path.lineTo(4.5f, 12f); path.lineTo(11f, 19f)
                path.moveTo(4.5f, 12f); path.lineTo(20f, 12f)
                c.drawPath(path, line)
            }
        }
        c.restore()
    }
}

/** The drawing surface. Touches go straight to the engine. */
class BoardView(context: Context) : SurfaceView(context) {
    var handle: Long = 0
    private var buffer = FloatArray(STRIDE * 64)

    /** Drawing buffer pixels per view pixel. Below 1 on very large panels. */
    var scale = 1f
        private set

    /** Longest side of the drawing buffer; 0 draws at the panel's full resolution. */
    var maxSide = MAX_SIDE
        set(value) {
            field = value
            fit(width, height)
        }

    /** Pointers being followed. Big IR panels report dozens; a few are enough. */
    private val followed = HashSet<Int>()

    private fun fit(w: Int, h: Int) {
        if (w <= 0 || h <= 0) return
        val longest = maxOf(w, h)
        scale = if (maxSide > 0 && longest > maxSide) maxSide.toFloat() / longest else 1f
        if (scale < 1f) holder.setFixedSize(Math.round(w * scale), Math.round(h * scale)) else holder.setSizeFromLayout()
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        fit(w, h)
    }

    private fun put(k: Int, e: MotionEvent, pointer: Int, x: Float, y: Float, time: Long, major: Float) {
        val o = k * STRIDE
        if (o + STRIDE > buffer.size) buffer = buffer.copyOf(buffer.size * 2)
        val tool = e.getToolType(pointer)
        buffer[o] = e.getPointerId(pointer).toFloat()
        buffer[o + 1] = x * scale
        buffer[o + 2] = y * scale
        buffer[o + 3] = (time - e.downTime).toFloat()
        buffer[o + 4] = major * scale
        buffer[o + 5] = if (tool == MotionEvent.TOOL_TYPE_STYLUS || tool == MotionEvent.TOOL_TYPE_ERASER) 1f else 0f
    }

    private fun one(action: Int, e: MotionEvent, pointer: Int) {
        put(0, e, pointer, e.getX(pointer), e.getY(pointer), e.eventTime, e.getTouchMajor(pointer))
        Native.touch(handle, action, e.downTime, buffer, 1)
    }

    override fun onTouchEvent(e: MotionEvent): Boolean {
        if (handle == 0L) return false
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                // Deliver samples as they arrive instead of once per frame.
                if (e.actionMasked == MotionEvent.ACTION_DOWN) {
                    requestUnbufferedDispatch(e)
                    followed.clear()
                }
                if (followed.size >= MAX_POINTERS) return true
                followed.add(e.getPointerId(e.actionIndex))
                one(0, e, e.actionIndex)
            }
            MotionEvent.ACTION_MOVE -> {
                var k = 0
                for (h in 0 until e.historySize) {
                    for (p in 0 until e.pointerCount) {
                        if (e.getPointerId(p) !in followed) continue
                        put(k++, e, p, e.getHistoricalX(p, h), e.getHistoricalY(p, h), e.getHistoricalEventTime(h), e.getHistoricalTouchMajor(p, h))
                    }
                }
                for (p in 0 until e.pointerCount) {
                    if (e.getPointerId(p) !in followed) continue
                    put(k++, e, p, e.getX(p), e.getY(p), e.eventTime, e.getTouchMajor(p))
                }
                if (k > 0) Native.touch(handle, 1, e.downTime, buffer, k)
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> {
                if (followed.remove(e.getPointerId(e.actionIndex))) one(2, e, e.actionIndex)
            }
            MotionEvent.ACTION_CANCEL -> {
                var k = 0
                for (p in 0 until e.pointerCount) {
                    if (e.getPointerId(p) in followed) put(k++, e, p, e.getX(p), e.getY(p), e.eventTime, e.getTouchMajor(p))
                }
                followed.clear()
                Native.touch(handle, 3, e.downTime, buffer, k)
            }
        }
        return true
    }

    companion object {
        const val STRIDE = 6
        const val MAX_SIDE = 1920
        const val MAX_POINTERS = 5
    }
}
