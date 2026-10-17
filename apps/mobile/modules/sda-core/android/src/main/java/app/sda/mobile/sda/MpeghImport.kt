package app.sda.mobile.sda

import android.content.Context
import android.net.Uri
import android.util.Base64
import org.json.JSONObject
import java.io.File
import java.io.RandomAccessFile
import java.util.UUID

/** Bounded file I/O only. The shared Windows Mp4Demuxer owns all MP4 parsing. */
class MpeghImport {
    private data class Session(val source: File, val output: File, val writer: java.io.OutputStream)
    private val sessions = mutableMapOf<String, Session>()

    @Synchronized fun begin(context: Context, uri: Uri): String {
        val source = File.createTempFile("sda-360ra-input-", ".mp4", context.cacheDir)
        var output: File? = null
        try {
            val input = context.contentResolver.openInputStream(uri) ?: error("无法打开所选音频文件")
            input.use { stream -> source.outputStream().use { stream.copyTo(it, 64 * 1024) } }
            output = File.createTempFile("sda-360ra-", ".mhas", context.cacheDir)
            val token = UUID.randomUUID().toString()
            sessions[token] = Session(source, output, output.outputStream().buffered(256 * 1024))
            return JSONObject().put("token", token).put("size", source.length()).toString()
        } catch (error: Throwable) {
            source.delete(); output?.delete(); throw error
        }
    }
    @Synchronized fun read(token: String, offset: Double, count: Int): String {
        require(offset.isFinite() && offset >= 0 && offset == offset.toLong().toDouble())
        require(count in 1..262144)
        val session = sessions[token] ?: error("媒体导入会话已关闭")
        RandomAccessFile(session.source, "r").use { file ->
            file.seek(offset.toLong())
            val bytes = ByteArray(count)
            val n = file.read(bytes)
            return if (n <= 0) "" else Base64.encodeToString(bytes, 0, n, Base64.NO_WRAP)
        }
    }
    @Synchronized fun append(token: String, base64: String) {
        require(base64.length <= 349528) { "导入数据块过大" }
        val session = sessions[token] ?: error("媒体导入会话已关闭")
        session.writer.write(Base64.decode(base64, Base64.NO_WRAP))
    }
    @Synchronized fun finish(token: String): String {
        val session = sessions[token] ?: error("媒体导入会话已关闭")
        session.writer.close()
        session.source.delete()
        require(session.output.length() > 0) { "360RA 音轨没有音频帧" }
        return Uri.fromFile(session.output).toString()
    }
    @Synchronized fun discard(token: String) {
        sessions.remove(token)?.let { session ->
            try { session.writer.close() } finally { session.source.delete(); session.output.delete() }
        }
    }
    @Synchronized fun close() { sessions.keys.toList().forEach { discard(it) } }
}
