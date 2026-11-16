package com.canvasstudio.rust

import android.app.Application
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.view.Surface

/** Process-wide text rasterisation bridge used for complex scripts. */
object Native {
    init { System.loadLibrary("canvas_android") }
    @JvmStatic external fun init(textClass: Class<*>)
}

/** Commands are queued to the Rust-owned application/render thread. */
object RustApp {
    init { System.loadLibrary("canvas_android") }
    @JvmStatic external fun create(host: MainActivity, dir: String, settings: String, version: String, utcOffset: Int): Long
    @JvmStatic external fun destroy(handle: Long)
    @JvmStatic external fun surface(handle: Long, surface: Surface?, w: Int, h: Int, dpr: Double)
    @JvmStatic external fun touch(handle: Long, action: Int, base: Long, data: FloatArray, count: Int)
    @JvmStatic external fun key(handle: Long, code: Int)
    @JvmStatic external fun back(handle: Long)
    @JvmStatic external fun textInput(handle: Long, text: String, submit: Boolean)
    @JvmStatic external fun inkResult(handle: Long, token: Int, text: String)
    @JvmStatic external fun updateResult(handle: Long, version: String, url: String, failed: Boolean, manual: Boolean)
    @JvmStatic external fun toast(handle: Long, text: String)
    @JvmStatic external fun pause(handle: Long)
}
/** Measures and draws text for the engine, which calls in from its render thread. */
object TextHost {
    const val LINE_HEIGHT = 1.3f
    val FONT_NAMES = arrayOf("Sans", "Serif", "Mono", "Handwriting", "Script")

    private val faces: Array<Typeface> by lazy {
        arrayOf(
            Typeface.SANS_SERIF,
            Typeface.SERIF,
            Typeface.MONOSPACE,
            Typeface.create("casual", Typeface.NORMAL),
            Typeface.create("cursive", Typeface.NORMAL),
        )
    }

    fun typeface(font: Int): Typeface = faces[font.coerceIn(0, faces.size - 1)]

    private fun paint(size: Float, font: Int) = Paint(Paint.ANTI_ALIAS_FLAG or Paint.SUBPIXEL_TEXT_FLAG).apply {
        textSize = size
        typeface = typeface(font)
    }

    @JvmStatic
    fun measure(text: String, fontSize: Float, font: Int): FloatArray {
        val p = paint(fontSize, font)
        val lines = text.split('\n')
        var widest = 0f
        for (line in lines) widest = maxOf(widest, p.measureText(line))
        return floatArrayOf(widest, lines.size * fontSize * LINE_HEIGHT)
    }

    @JvmStatic
    fun render(text: String, fontPx: Float, color: Int, font: Int): IntArray? {
        val p = paint(fontPx, font)
        p.color = color
        val lines = text.split('\n')
        var widest = 0f
        for (line in lines) widest = maxOf(widest, p.measureText(line))
        val lineHeight = fontPx * LINE_HEIGHT
        // Slanted and script faces overhang their advance width a little.
        val w = (Math.ceil(widest.toDouble()).toInt() + Math.ceil(fontPx * 0.25).toInt()).coerceIn(1, 4096)
        val h = Math.ceil((lines.size * lineHeight).toDouble()).toInt().coerceIn(1, 4096)
        val bitmap = try {
            Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
        } catch (e: OutOfMemoryError) {
            return null
        }
        val canvas = Canvas(bitmap)
        val fm = p.fontMetrics
        val inset = (lineHeight - (fm.descent - fm.ascent)) / 2f - fm.ascent
        for ((i, line) in lines.withIndex()) canvas.drawText(line, 0f, i * lineHeight + inset, p)
        val out = IntArray(w * h + 2)
        out[0] = w
        out[1] = h
        bitmap.getPixels(out, 2, w, 0, 0, w, h)
        bitmap.recycle()
        return out
    }
}

class App : Application() {
    override fun onCreate() {
        super.onCreate()
        Native.init(TextHost::class.java)
    }
}
