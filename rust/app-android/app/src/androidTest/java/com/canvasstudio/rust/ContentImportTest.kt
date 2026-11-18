package com.canvasstudio.rust

import android.graphics.BitmapFactory
import android.graphics.Color
import android.graphics.pdf.PdfDocument
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

@RunWith(AndroidJUnit4::class)
class ContentImportTest {
    @Test
    fun rendersGeneratedPdfInPageOrder() {
        val cache = InstrumentationRegistry.getInstrumentation().targetContext.cacheDir
        val file = File.createTempFile("pdf-import-test-", ".pdf", cache)
        val colors = listOf(Color.RED, Color.BLUE)
        try {
            val document = PdfDocument()
            try {
                colors.forEachIndexed { index, color ->
                    val page = document.startPage(PdfDocument.PageInfo.Builder(72, 144, index + 1).create())
                    page.canvas.drawColor(color)
                    document.finishPage(page)
                }
                file.outputStream().use { document.writeTo(it) }
            } finally {
                document.close()
            }
            val pages = ContentImport.rasterize(file)
            assertEquals(2, pages.size)
            pages.forEachIndexed { index, bytes ->
                val bitmap = BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
                try {
                    assertEquals(2 * bitmap.width, bitmap.height)
                    assertTrue(bitmap.height <= 1920)
                    assertEquals(colors[index], bitmap.getPixel(bitmap.width / 2, bitmap.height / 2))
                } finally {
                    bitmap.recycle()
                }
            }
        } finally {
            file.delete()
        }
    }
}
