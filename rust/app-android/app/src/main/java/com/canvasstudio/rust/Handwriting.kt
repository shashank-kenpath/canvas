package com.canvasstudio.rust

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import com.google.android.gms.tasks.Task
import com.google.mlkit.common.model.DownloadConditions
import com.google.mlkit.common.model.RemoteModelManager
import com.google.mlkit.vision.digitalink.recognition.DigitalInkRecognition
import com.google.mlkit.vision.digitalink.recognition.DigitalInkRecognitionModel
import com.google.mlkit.vision.digitalink.recognition.DigitalInkRecognitionModelIdentifier
import com.google.mlkit.vision.digitalink.recognition.DigitalInkRecognizer
import com.google.mlkit.vision.digitalink.recognition.DigitalInkRecognizerOptions
import com.google.mlkit.vision.digitalink.recognition.Ink
import org.json.JSONArray

/** Thin ML Kit bridge. Language/settings/replacement decisions belong to Rust. */
class Handwriting(private val context: Context) {
    private class Model(val client: DigitalInkRecognizer, var download: Task<Void>?)
    private val models = HashMap<String, Model>()
    private val main = Handler(Looper.getMainLooper())
    private var closed = false

    private fun language(lang: String): String = if (lang == "hi" || lang.startsWith("hi-")) "hi" else "en-US"
    private fun playServicesPresent(): Boolean = try {
        context.packageManager.getApplicationInfo("com.google.android.gms", 0).enabled
    } catch (_: Exception) { false }

    /** Called only when the Rust setting requests handwriting recognition. */
    fun prepare(lang: String) {
        if (closed || !playServicesPresent()) return
        val tag = language(lang)
        val old = models[tag]
        if (old?.download?.isSuccessful == true || old?.download?.isComplete == false) return
        try {
            val id = DigitalInkRecognitionModelIdentifier.fromLanguageTag(tag) ?: return
            val model = DigitalInkRecognitionModel.builder(id).build()
            val entry = old ?: Model(DigitalInkRecognition.getClient(DigitalInkRecognizerOptions.builder(model).build()), null)
            models[tag] = entry
            entry.download = RemoteModelManager.getInstance().download(model, DownloadConditions.Builder().build())
                .addOnFailureListener { Log.w(TAG, "Handwriting model is unavailable", it) }
        } catch (error: Exception) { Log.w(TAG, "Handwriting is unavailable", error) }
    }

    /** Wire format: token, stroke count, then point count and x/y pairs. */
    fun read(lang: String, request: FloatArray, done: (String) -> Unit) {
        prepare(lang)
        val entry = models[language(lang)]
        val pending = entry?.download
        if (closed || entry == null || pending == null || request.size < 3) { done(""); return }
        val built = try {
            val count = request[1].toInt()
            require(count in 1..4096)
            val ink = Ink.builder()
            var i = 2
            var time = 0L
            for (s in 0 until count) {
                require(i < request.size)
                val n = request[i++].toInt()
                require(n in 1..100_000 && n <= (request.size - i) / 2)
                val stroke = Ink.Stroke.builder()
                repeat(n) {
                    val x = request[i++]; val y = request[i++]
                    require(x.isFinite() && y.isFinite())
                    stroke.addPoint(Ink.Point.create(x, y, time)); time += 8
                }
                ink.addStroke(stroke.build()); time += 150
            }
            ink.build()
        } catch (_: Exception) { done(""); return }

        var answered = false
        val complete: (String) -> Unit = { text ->
            if (!answered) { answered = true; done(if (closed) "" else text) }
        }
        val timeout = Runnable { complete("") }
        main.postDelayed(timeout, 120_000)
        pending.addOnCompleteListener { result ->
            if (closed || !result.isSuccessful) { main.removeCallbacks(timeout); complete("") }
            else entry.client.recognize(built)
                .addOnSuccessListener { recognized ->
                    main.removeCallbacks(timeout)
                    val candidates = JSONArray()
                    recognized.candidates.take(5).forEach { candidates.put(it.text) }
                    complete(candidates.toString())
                }
                .addOnFailureListener { error -> Log.w(TAG, "Handwriting could not be read", error); main.removeCallbacks(timeout); complete("") }
        }
    }

    fun close() {
        closed = true
        main.removeCallbacksAndMessages(null)
        models.values.forEach { it.client.close() }
        models.clear()
    }
    companion object { const val TAG = "CanvasRust" }
}
