package app.sda.mobile.sda

import android.graphics.Bitmap
import org.junit.Assert.*
import org.junit.Test

class ArtworkColorTest {
    @Test fun samplesBottomInsteadOfTopWithoutPlayback() {
        val bitmap = Bitmap.createBitmap(100, 100, Bitmap.Config.ARGB_8888)
        try {
            bitmap.eraseColor(0xffff0000.toInt())
            for (y in 70..99) for (x in 0..99) bitmap.setPixel(x, y, 0xff0000ff.toInt())
            assertEquals("rgb(0,0,133)", ArtworkColor.bottomBand(bitmap))
            bitmap.eraseColor(0)
            assertNull(ArtworkColor.bottomBand(bitmap))
        } finally { bitmap.recycle() }
    }
    @Test fun handlesTinyCoversAndAlpha() {
        val bitmap = Bitmap.createBitmap(1, 1, Bitmap.Config.ARGB_8888)
        try {
            bitmap.eraseColor(0xff646464.toInt())
            assertEquals("rgb(52,52,52)", ArtworkColor.bottomBand(bitmap))
            bitmap.eraseColor(0x800000ff.toInt())
            assertEquals("rgb(0,0,133)", ArtworkColor.bottomBand(bitmap))
        } finally { bitmap.recycle() }
    }
}
