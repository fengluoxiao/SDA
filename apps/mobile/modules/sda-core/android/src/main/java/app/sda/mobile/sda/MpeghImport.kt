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
    private data class Session(val source: File, val ownsSource: Boolean, val output: File, var writer: java.io.OutputStream?, var finished: Boolean = false)
    private val sessions = mutableMapOf<String, Session>()
    private var cleaned = false
    companion object {
        /** Only a copied picker file owned by this app may be replaced by its raw track. */
        fun discardPickerCopy(context: Context, uri: Uri): Boolean {
            if (uri.scheme != "file") return false
            val file = uri.path?.let { File(it).canonicalFile } ?: return false
            val picker = File(context.cacheDir, "DocumentPicker").canonicalFile
            if (file.parentFile != picker || !file.isFile) return false
            return file.delete()
        }
    }
    private fun cleanStale(context: Context) {
        if (cleaned) return
        cleaned = true
        val pid = android.os.Process.myPid()
        context.cacheDir.listFiles()?.filter { file ->
            file.isFile && file.name.startsWith("sda-360ra-") &&
                (file.name.endsWith(".mhas") || file.name.endsWith(".mp4")) &&
                !file.name.startsWith("sda-360ra-$pid-") && !file.name.startsWith("sda-360ra-input-$pid-")
        }?.forEach { it.delete() }
    }

    @Synchronized fun begin(context: Context, uri: Uri): String {
        cleanStale(context)
        val existing = if (uri.scheme == "file") uri.path?.let { File(it) }?.takeIf { it.isFile } else null
        val ownsSource = existing == null
        val source = existing ?: File.createTempFile("sda-360ra-input-${android.os.Process.myPid()}-", ".mp4", context.cacheDir)
        var output: File? = null
        try {
            if (ownsSource) {
                val input = context.contentResolver.openInputStream(uri) ?: error("无法打开所选音频文件")
                input.use { stream -> source.outputStream().use { stream.copyTo(it, 64 * 1024) } }
            }
            output = File.createTempFile("sda-360ra-${android.os.Process.myPid()}-", ".mhas", context.cacheDir)
            val token = UUID.randomUUID().toString()
            sessions[token] = Session(source, ownsSource, output, output.outputStream().buffered(256 * 1024))
            return JSONObject().put("token", token).put("size", source.length()).toString()
        } catch (error: Throwable) {
            if (ownsSource) source.delete()
            output?.delete(); throw error
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
        check(!session.finished) { "音轨已经完成准备" }
        (session.writer ?: error("音轨写入已关闭")).write(Base64.decode(base64, Base64.NO_WRAP))
    }
    @Synchronized fun finish(token: String): String {
        val session = sessions[token] ?: error("媒体导入会话已关闭")
        session.writer?.close()
        session.writer = null
        session.finished = true
        if (session.ownsSource) session.source.delete()
        require(session.output.length() > 0) { "360RA 音轨没有音频帧" }
        return Uri.fromFile(session.output).toString()
    }
    @Synchronized fun adopt(context: Context, token: String) {
        val session = sessions[token] ?: error("媒体导入会话已关闭")
        check(session.finished && session.output.length() > 0) { "音轨尚未准备完成" }
        if (!session.ownsSource) discardPickerCopy(context, Uri.fromFile(session.source))
    }
    @Synchronized fun discard(token: String) {
        sessions.remove(token)?.let { session ->
            try { session.writer?.close() } finally { if (session.ownsSource) session.source.delete(); session.output.delete() }
        }
    }
    @Synchronized fun close() { sessions.keys.toList().forEach { discard(it) } }
}
