package app.sda.mobile.sda

import android.content.Context
import android.media.MediaExtractor
import android.media.MediaFormat
import android.net.Uri
import java.io.InputStream
import java.nio.ByteBuffer
import java.util.Locale

/** Extracts compressed E-AC-3 access units without decoding or discarding JOC metadata. */
object Eac3Input {
    /** Duration of the selected compressed audio track, never inferred from decode-ahead. */
    fun durationMs(context: Context, uri: Uri): Double {
        val extractor = MediaExtractor()
        return try {
            extractor.setDataSource(context, uri, null)
            val format = (0 until extractor.trackCount).map { extractor.getTrackFormat(it) }
                .firstOrNull { it.getString(MediaFormat.KEY_MIME) in setOf("audio/eac3", "audio/eac3-joc") }
            if (format != null && format.containsKey(MediaFormat.KEY_DURATION))
                maxOf(0L, format.getLong(MediaFormat.KEY_DURATION)).toDouble() / 1000.0
            else 0.0
        } catch (_: Exception) {
            // Some raw streams have no container duration; playback remains available.
            0.0
        } finally {
            extractor.release()
        }
    }

    fun open(context: Context, uri: Uri, displayName: String): InputStream {
        return when (displayName.substringAfterLast('.', "").lowercase(Locale.ROOT)) {
            "eac3", "ec3", "mhas" -> context.contentResolver.openInputStream(uri)
                ?: throw IllegalArgumentException("无法打开所选音频文件")
            "m4a", "mp4" -> openContainer(context, uri)
            else -> throw IllegalArgumentException("支持 .eac3/.ec3、.mhas，以及包含 Atmos/360RA 音轨的 .m4a/.mp4")
        }
    }

    private fun openContainer(context: Context, uri: Uri): InputStream {
        val extractor = MediaExtractor()
        try {
            extractor.setDataSource(context, uri, null)
            val formats = (0 until extractor.trackCount).map { extractor.getTrackFormat(it) }
            val track = formats.indexOfFirst {
                it.getString(MediaFormat.KEY_MIME) in setOf("audio/eac3", "audio/eac3-joc")
            }
            require(track >= 0) {
                val codecs = formats.mapNotNull { it.getString(MediaFormat.KEY_MIME) }.joinToString()
                "文件中没有可播放的 E-AC-3/Atmos 音轨（检测到：${codecs.ifBlank { "无音轨" }}）"
            }
            extractor.selectTrack(track)
            return ExtractedStream(extractor)
        } catch (error: Throwable) {
            extractor.release()
            throw error
        }
    }

    private class ExtractedStream(private val extractor: MediaExtractor) : InputStream() {
        // E-AC-3 samples are normally a few KB. Bound memory independently of file duration.
        private val packet = ByteBuffer.allocate(1024 * 1024).apply { limit(0) }
        private var closed = false

        @Synchronized
        override fun read(bytes: ByteArray, offset: Int, length: Int): Int {
            check(!closed) { "音频文件已关闭" }
            require(offset >= 0 && length >= 0 && offset <= bytes.size - length)
            if (length == 0) return 0
            while (!packet.hasRemaining()) {
                if (extractor.sampleTrackIndex < 0) return -1
                require(extractor.sampleFlags and MediaExtractor.SAMPLE_FLAG_ENCRYPTED == 0) {
                    "不支持加密音轨，请选择未加密的 E-AC-3/Atmos 文件"
                }
                packet.clear()
                val count = extractor.readSampleData(packet, 0)
                if (count < 0) {
                    packet.limit(0)
                    return -1
                }
                check(count <= packet.capacity()) { "E-AC-3 音频帧过大" }
                packet.position(0)
                packet.limit(count)
                extractor.advance()
            }
            val count = minOf(length, packet.remaining())
            packet.get(bytes, offset, count)
            return count
        }

        override fun read(): Int {
            val byte = ByteArray(1)
            return if (read(byte, 0, 1) < 0) -1 else byte[0].toInt() and 0xff
        }

        @Synchronized
        override fun close() {
            if (!closed) {
                closed = true
                extractor.release()
            }
        }
    }
}
