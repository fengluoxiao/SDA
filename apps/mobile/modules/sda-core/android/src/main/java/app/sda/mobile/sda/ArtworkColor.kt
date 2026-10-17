package app.sda.mobile.sda

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Rect
import kotlin.math.roundToInt

/** Small optional metadata sample, independent of playback and OpenGL. */
object ArtworkColor {
    fun bottomBand(bitmap: Bitmap): String? {
        if (bitmap.width <= 0 || bitmap.height <= 0) return null
        val top = (bitmap.height * 0.70).toInt().coerceAtMost(bitmap.height - 1)
        val bottom = (top + (bitmap.height * 0.29).toInt().coerceAtLeast(1)).coerceAtMost(bitmap.height)
        val sample = Bitmap.createBitmap(16, 16, Bitmap.Config.ARGB_8888)
        return try {
            Canvas(sample).drawBitmap(bitmap, Rect(0, top, bitmap.width, bottom), Rect(0, 0, 16, 16), Paint(Paint.FILTER_BITMAP_FLAG))
            val pixels = IntArray(256)
            sample.getPixels(pixels, 0, 16, 0, 0, 16, 16)
            averageArgb(pixels)
        } finally { sample.recycle() }
    }

    internal fun averageArgb(pixels: IntArray): String? {
        val sums = DoubleArray(3)
        var weight = 0.0
        for (pixel in pixels) {
            val alpha = ((pixel ushr 24) and 255) / 255.0
            weight += alpha
            sums[0] += ((pixel ushr 16) and 255) * alpha
            sums[1] += ((pixel ushr 8) and 255) * alpha
            sums[2] += (pixel and 255) * alpha
        }
        if (weight < 1.0) return null
        val rgb = sums.map { (it / weight * 0.52).roundToInt().coerceIn(0, 255) }
        return "rgb(${rgb.joinToString(",")})"
    }
}
