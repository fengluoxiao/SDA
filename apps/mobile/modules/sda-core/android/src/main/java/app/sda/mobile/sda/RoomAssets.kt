package app.sda.mobile.sda

import android.content.Context
import java.io.File
import java.security.MessageDigest
import java.util.zip.GZIPInputStream
import org.json.JSONArray
import org.json.JSONObject

/** Verified Windows room responses; the native renderer owns all room DSP. */
internal object RoomAssets {
    private fun entries(context: Context): JSONArray = JSONObject(
        context.assets.open("builtin-rooms/catalog.json").bufferedReader().use { it.readText() }
    ).getJSONArray("profiles")

    fun list(context: Context): JSONArray {
        val entries = entries(context)
        return JSONArray().apply {
            for (i in 0 until entries.length()) {
                val summary = entries.getJSONObject(i).getJSONObject("summary")
                put(JSONObject().put("id", summary.getString("id"))
                    .put("name", summary.getString("name"))
                    .put("layout", summary.getString("layout")))
            }
        }
    }

    /** Same selection order as Windows room-layout-follow.cjs: keep disabled,
     * keep matching, restore remembered, then select the matching builtin. */
    fun forLayout(context: Context, id: String, layout: String): String {
        if (id.isEmpty()) return ""
        val catalog = entries(context)
        fun matching(candidate: String) = (0 until catalog.length()).any {
            val entry = catalog.getJSONObject(it)
            entry.getString("id") == candidate && entry.getJSONObject("summary").getString("layout") == layout
        }
        if (matching(id)) return id
        val remembered = context.getSharedPreferences("sda-rendering", 0).getString("roomId_$layout", "") ?: ""
        if (matching(remembered)) return remembered
        return (0 until catalog.length()).map { catalog.getJSONObject(it) }
            .firstOrNull { it.getJSONObject("summary").getString("layout") == layout }?.getString("id")
            ?: error("没有与 $layout 匹配的 Windows 房间资产")
    }

    fun layout(context: Context, id: String): String = (0 until entries(context).length())
        .map { entries(context).getJSONObject(it) }.first { it.getString("id") == id }
        .getJSONObject("summary").getString("layout")

    private fun hash(bytes: ByteArray) = MessageDigest.getInstance("SHA-256")
        .digest(bytes).joinToString("") { "%02x".format(it) }

    fun prepare(context: Context, id: String): String {
        if (id.isEmpty()) return ""
        val entries = entries(context)
        val entry = (0 until entries.length()).map { entries.getJSONObject(it) }
            .firstOrNull { it.getString("id") == id } ?: error("未知房间资产")
        val directory = File(context.filesDir, "builtin-rooms").apply { mkdirs() }
        val file = File(directory, "$id.json")
        if (file.isFile && file.length() == entry.getLong("bytes") && hash(file.readBytes()) == id) return file.absolutePath
        val compressed = context.assets.open("builtin-rooms/${entry.getString("file")}.bin").use { it.readBytes() }
        check(hash(compressed) == entry.getString("compressedSha256")) { "房间压缩资产校验失败" }
        val bytes = GZIPInputStream(compressed.inputStream()).use { it.readBytes() }
        check(bytes.size.toLong() == entry.getLong("bytes") && hash(bytes) == id) { "房间资产校验失败" }
        val temp = File(directory, "$id.tmp")
        temp.writeBytes(bytes)
        check(temp.renameTo(file)) { "无法保存房间资产" }
        return file.absolutePath
    }
}
