package app.sda.mobile.sda

import android.net.Uri
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.security.MessageDigest
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test

class Eac3InputTest {
    private val context get() = InstrumentationRegistry.getInstrumentation().context

    @Test
    fun containerMatchesDesktopDemuxBytes() {
        val args = InstrumentationRegistry.getArguments()
        val expected = args.getString("eac3Sha256")
        assumeTrue("Supply a local fixture and its desktop-demux SHA-256", expected != null)
        val fixture = File(context.getExternalFilesDir(null), "sample.m4a")
        assertTrue("Push sample.m4a into the test app external files directory", fixture.isFile)
        val expectedDuration = args.getString("durationMs")?.toDouble()
        if (expectedDuration != null) {
            assertEquals(expectedDuration, Eac3Input.durationMs(context, Uri.fromFile(fixture)), 1.0)
        }
        if (args.getString("verifyDollMetadata") == "true") {
            val metadata = MediaMetadata.read(context, Uri.fromFile(fixture))
            assertEquals("doll", metadata.getString("title"))
            assertEquals("陈康堤", metadata.getString("artist"))
            assertEquals("doll - Single", metadata.getString("album"))
            val cover = File(Uri.parse(metadata.getString("coverUri")).path!!)
            assertTrue(cover.isFile && cover.length() > 0)
            val bitmap = android.graphics.BitmapFactory.decodeFile(cover.path)
            assertNotNull(bitmap)
            assertTrue(bitmap.width <= 1024 && bitmap.height <= 1024)
            bitmap.recycle()
        }
        val digest = MessageDigest.getInstance("SHA-256")
        var total = 0L
        Eac3Input.open(context, Uri.fromFile(fixture), fixture.name).use { input ->
            // Deliberately split access units across reads to exercise byte-stream semantics.
            val buffer = ByteArray(997)
            assertEquals(0, input.read(buffer, 3, 0))
            while (true) {
                val count = input.read(buffer, 3, 991)
                if (count < 0) break
                digest.update(buffer, 3, count)
                total += count
            }
            assertEquals(-1, input.read())
            assertEquals(-1, input.read(buffer))
        }
        assertTrue(total > 0)
        assertEquals(expected, digest.digest().joinToString("") { "%02x".format(it) })
    }

    @Test
    fun contentIdentitySurvivesPickerCacheCopies() {
        val first = File.createTempFile("picker-first", ".m4a", context.cacheDir)
        val second = File.createTempFile("picker-second", ".m4a", context.cacheDir)
        try {
            val bytes = ByteArray(131073) { (it * 31).toByte() }
            first.writeBytes(bytes)
            second.writeBytes(bytes)
            val hash = MediaMetadata.contentHash(context, Uri.fromFile(first))
            assertEquals(MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }, hash)
            assertEquals(hash, MediaMetadata.contentHash(context, Uri.fromFile(second)))
            bytes[bytes.lastIndex] = (bytes.last() + 1).toByte()
            second.writeBytes(bytes)
            assertNotEquals(hash, MediaMetadata.contentHash(context, Uri.fromFile(second)))
        } finally {
            first.delete()
            second.delete()
        }
    }

    @Test
    fun rawStreamIsUnmodified() {
        val file = File.createTempFile("raw", ".ec3", context.cacheDir)
        try {
            val bytes = ByteArray(8193) { (it * 31).toByte() }
            file.writeBytes(bytes)
            Eac3Input.open(context, Uri.fromFile(file), file.name).use {
                assertArrayEquals(bytes, it.readBytes())
            }
        } finally {
            file.delete()
        }
    }

    @Test
    fun containerWithoutEac3IsRejected() {
        val file = File.createTempFile("pcm", ".m4a", context.cacheDir)
        try {
            // A valid PCM WAV with a misleading extension must not reach the E-AC-3 decoder.
            val wav = java.nio.ByteBuffer.allocate(46).order(java.nio.ByteOrder.LITTLE_ENDIAN)
            wav.put("RIFF".toByteArray()).putInt(38).put("WAVEfmt ".toByteArray())
            wav.putInt(16).putShort(1.toShort()).putShort(1.toShort()).putInt(48000)
            wav.putInt(96000).putShort(2.toShort()).putShort(16.toShort())
            wav.put("data".toByteArray()).putInt(2).putShort(0.toShort())
            file.writeBytes(wav.array())
            try {
                Eac3Input.open(context, Uri.fromFile(file), file.name).close()
                fail("PCM must not be accepted as E-AC-3")
            } catch (error: IllegalArgumentException) {
                assertTrue(error.message.orEmpty().contains("没有可播放的 E-AC-3"))
            }
        } finally {
            file.delete()
        }
    }
}
