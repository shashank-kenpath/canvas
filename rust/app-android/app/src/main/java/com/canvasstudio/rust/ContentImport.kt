package com.canvasstudio.rust

import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.Matrix
import android.graphics.pdf.PdfRenderer
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.io.OutputStream
import kotlin.math.max
import kotlin.math.roundToInt
import kotlin.math.sqrt

internal object ContentImport {
    private const val PDF = 1
    private const val FILE_LIMIT = 32 * 1024 * 1024
    private const val PDF_LIMIT = 32 * 1024 * 1024
    private const val PAGE_LIMIT = 80
    private const val PAGE_SIDE = 1920
    private const val PAGE_PIXELS = 2_500_000.0

    data class Content(val name: String, val mime: String, val bytes: Array<ByteArray>)

    fun intent(kind: Int): Intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
        addCategory(Intent.CATEGORY_OPENABLE)
        type = "*/*"
        putExtra(Intent.EXTRA_MIME_TYPES, when (kind) {
            0 -> arrayOf("image/png", "image/jpeg", "image/webp", "image/gif", "image/bmp", "image/x-ms-bmp", "application/octet-stream")
            PDF -> arrayOf("application/pdf", "application/octet-stream")
            else -> arrayOf(
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                "image/svg+xml", "application/zip", "application/octet-stream"
            )
        })
    }

    fun read(context: Context, uri: Uri, kind: Int): Content {
        val resolver = context.contentResolver
        val mime = resolver.getType(uri).orEmpty()
        val name = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
            if (it.moveToFirst()) it.getString(0) else null
        } ?: uri.lastPathSegment.orEmpty()
        if (kind == PDF || mime == "application/pdf" || name.endsWith(".pdf", ignoreCase = true)) {
            // Some document providers expose streams or pipes. PdfRenderer requires a seekable file.
            val file = File.createTempFile("insert-", ".pdf", context.cacheDir)
            try {
                file.outputStream().use { copy(context, uri, it) }
                return Content(name, "application/pdf", rasterize(file))
            } finally {
                file.delete()
            }
        }
        val output = ByteArrayOutputStream()
        copy(context, uri, output)
        return Content(name, mime, arrayOf(output.toByteArray()))
    }

    private fun copy(context: Context, uri: Uri, output: OutputStream) {
        val stream = context.contentResolver.openInputStream(uri) ?: throw IOException("Could not open the file")
        stream.use { input ->
            val buffer = ByteArray(16 * 1024)
            var total = 0
            while (true) {
                val count = input.read(buffer)
                if (count == -1) break
                total += count
                if (total > FILE_LIMIT) throw IOException("Files must be 32 MiB or smaller")
                output.write(buffer, 0, count)
            }
        }
    }

    fun rasterize(file: File): Array<ByteArray> {
        val descriptor = ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
        val renderer = try {
            PdfRenderer(descriptor)
        } catch (e: Exception) {
            descriptor.close()
            throw IOException("Could not open this PDF", e)
        }
        renderer.use { pdf ->
            if (pdf.pageCount !in 1..PAGE_LIMIT) {
                throw IOException("Inkboard accepts PDFs with 1 to $PAGE_LIMIT pages")
            }
            var remaining = PDF_LIMIT
            return Array(pdf.pageCount) { index ->
                pdf.openPage(index).use { page ->
                    val scale = minOf(
                        PAGE_SIDE.toDouble() / max(page.width, page.height),
                        sqrt(PAGE_PIXELS / (page.width.toDouble() * page.height))
                    ).toFloat()
                    val width = (page.width * scale).roundToInt().coerceAtLeast(1)
                    val height = (page.height * scale).roundToInt().coerceAtLeast(1)
                    val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
                    try {
                        bitmap.eraseColor(Color.WHITE)
                        val matrix = Matrix().apply { setScale(scale, scale) }
                        page.render(bitmap, null, matrix, PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)
                        val output = ByteArrayOutputStream()
                        if (!bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)) {
                            throw IOException("Could not rasterize PDF page ${index + 1}")
                        }
                        remaining -= output.size()
                        if (remaining < 0) throw IOException("The rendered PDF exceeds Inkboard's size limit")
                        output.toByteArray()
                    } finally {
                        bitmap.recycle()
                    }
                }
            }
        }
    }
}
