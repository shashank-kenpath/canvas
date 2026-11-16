package com.canvasstudio.rust

import android.app.Application
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.view.Surface

/** The Rust engine. See rust/android/src/lib.rs for what each call does. */
object Native {
    init {
        System.loadLibrary("canvas_android")
    }

    /** Called on the render thread with a set of EV_ bits. */
    interface Listener {
        fun onEvent(bits: Int)
    }

    const val EV_STATE = 1
    const val EV_DOC = 2
    const val EV_TEXT = 4
    const val EV_HAPTIC = 8
    const val EV_VIEW = 16
    const val EV_INK = 32

    @JvmStatic external fun init(textClass: Class<*>)
    @JvmStatic external fun create(listener: Listener): Long
    @JvmStatic external fun destroy(handle: Long)
    @JvmStatic external fun surface(handle: Long, surface: Surface?, w: Int, h: Int, dpr: Double)
    @JvmStatic external fun touch(handle: Long, action: Int, base: Long, data: FloatArray, count: Int)
    @JvmStatic external fun command(handle: Long, command: Int, a: Double, b: Double)
    @JvmStatic external fun state(handle: Long): DoubleArray?
    @JvmStatic external fun takeTextRequest(handle: Long): String?
    @JvmStatic external fun commitText(handle: Long, id: String, x: Double, y: Double, content: String)
    @JvmStatic external fun takeInkRequest(handle: Long): FloatArray?
    @JvmStatic external fun commitInk(handle: Long, token: Int, content: String)
    @JvmStatic external fun loadBoard(handle: Long, dir: String, id: String): String?
    @JvmStatic external fun saveBoard(handle: Long, dir: String, id: String, title: String, updated: String, evenIfDrawing: Int): Int
    @JvmStatic external fun listBoards(dir: String): String?
    @JvmStatic external fun createBoard(dir: String, id: String, title: String, updated: String): Int
    @JvmStatic external fun deleteBoard(dir: String, id: String): Int
    @JvmStatic external fun renameBoard(dir: String, id: String, title: String, updated: String): Int
    @JvmStatic external fun thumbnail(dir: String, id: String, w: Int, h: Int): IntArray?
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
