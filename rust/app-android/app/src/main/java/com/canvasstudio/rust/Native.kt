package com.canvasstudio.rust

import android.app.Application
import android.content.res.AssetManager
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
    /** Same order as `text::FONTS` in the engine; bundled faces live in assets/fonts. */
    private val FONT_IDS = arrayOf(
        "sans", "serif", "mono", "shantell", "dancing", "inter", "opensans", "lato", "montserrat", "poppins", "nunito",
        "oswald", "raleway", "ubuntu", "merriweather", "playfair", "lora", "robotoslab", "jetbrains", "sourcecode",
        "caveat", "patrick", "kalam", "indie", "architects", "shadows", "comicneue", "marker", "amatic", "pacifico",
        "greatvibes", "lobster", "bebas", "notodeva", "hind", "tiro",
    )
    @Volatile var assets: AssetManager? = null
    private val faces = arrayOfNulls<Typeface>(FONT_IDS.size)

    fun typeface(font: Int): Typeface {
        val i = font.coerceIn(0, FONT_IDS.size - 1)
        faces[i]?.let { return it }
        val face = when (FONT_IDS[i]) {
            "sans" -> Typeface.SANS_SERIF
            "serif" -> Typeface.SERIF
            "mono" -> Typeface.MONOSPACE
            else -> try {
                Typeface.createFromAsset(assets, "fonts/${FONT_IDS[i]}.ttf")
            } catch (e: Exception) {
                Typeface.SANS_SERIF
            }
        }
        faces[i] = face
        return face
    }

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
        TextHost.assets = assets
        Native.init(TextHost::class.java)
    }
}
