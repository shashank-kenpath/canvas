package com.canvasstudio.rust

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

/** Reads handwriting on the device. The model is downloaded once, on first use. */
class Handwriting {
    private var recognizer: DigitalInkRecognizer? = null
    private var download: Task<Void>? = null

    /** True once the model is on the device. */
    @Volatile
    var ready = false
        private set

    /** Starts fetching the model if it is not here yet. Safe to call again after a failure. */
    fun prepare() {
        if (ready || download?.isComplete == false) return
        try {
            val id = DigitalInkRecognitionModelIdentifier.fromLanguageTag("en-US") ?: return
            val model = DigitalInkRecognitionModel.builder(id).build()
            recognizer = recognizer ?: DigitalInkRecognition.getClient(DigitalInkRecognizerOptions.builder(model).build())
            download = RemoteModelManager.getInstance().download(model, DownloadConditions.Builder().build())
                .addOnSuccessListener { ready = true }
                .addOnFailureListener { Log.w(TAG, "handwriting model download failed", it) }
        } catch (e: Exception) {
            Log.w(TAG, "handwriting is unavailable", e)
        }
    }

    /**
     * Reads a request from [Native.takeInkRequest] and calls [done] on the main
     * thread with the text, or with an empty string if it could not be read.
     */
    fun read(request: FloatArray, done: (String) -> Unit) {
        val client = recognizer
        val pending = download
        if (client == null || pending == null) {
            prepare()
            done("")
            return
        }
        val ink = Ink.builder()
        var i = 2
        var t = 0L
        val strokes = request[1].toInt()
        for (s in 0 until strokes) {
            if (i >= request.size) break
            val n = request[i++].toInt()
            val stroke = Ink.Stroke.builder()
            for (p in 0 until n) {
                if (i + 1 >= request.size) break
                stroke.addPoint(Ink.Point.create(request[i], request[i + 1], t))
                t += 8
                i += 2
            }
            ink.addStroke(stroke.build())
            t += 150
        }
        val built = ink.build()
        pending.addOnCompleteListener { result ->
            if (!result.isSuccessful) {
                prepare()
                done("")
                return@addOnCompleteListener
            }
            client.recognize(built)
                .addOnSuccessListener { r -> done(r.candidates.firstOrNull()?.text ?: "") }
                .addOnFailureListener {
                    Log.w(TAG, "handwriting was not read", it)
                    done("")
                }
        }
    }

    fun close() {
        recognizer?.close()
        recognizer = null
    }

    companion object {
        const val TAG = "CanvasRust"
    }
}
