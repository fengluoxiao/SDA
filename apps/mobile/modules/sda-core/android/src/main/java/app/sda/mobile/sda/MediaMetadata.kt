package app.sda.mobile.sda

import android.content.Context
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import java.io.File
import java.security.MessageDigest
import org.json.JSONObject

object MediaMetadata {
    // Picker cache URIs change on every import; identify the actual file contents.
    fun contentHash(context: Context, uri: Uri): String {
        val digest = MessageDigest.getInstance("SHA-256")
        val input = context.contentResolver.openInputStream(uri)
            ?: throw IllegalArgumentException("无法读取所选媒体文件")
        input.use {
            val buffer = ByteArray(64 * 1024)
            while (true) {
                val count = it.read(buffer)
                if (count < 0) break
                if (count > 0) digest.update(buffer, 0, count)
            }
        }
        return digest.digest().joinToString("") { "%02x".format(it) }
    }

    fun read(context: Context, uri: Uri): JSONObject {
        val result = JSONObject().put("durationMs", Eac3Input.durationMs(context, uri))
        val retriever = MediaMetadataRetriever()
        try {
            retriever.setDataSource(context, uri)
            val keys = mapOf(
                "title" to MediaMetadataRetriever.METADATA_KEY_TITLE,
                "artist" to MediaMetadataRetriever.METADATA_KEY_ARTIST,
                "album" to MediaMetadataRetriever.METADATA_KEY_ALBUM,
                "albumArtist" to MediaMetadataRetriever.METADATA_KEY_ALBUMARTIST,
                "year" to MediaMetadataRetriever.METADATA_KEY_YEAR,
                "track" to MediaMetadataRetriever.METADATA_KEY_CD_TRACK_NUMBER
            )
            keys.forEach { (name, key) ->
                retriever.extractMetadata(key)?.trim()?.takeIf { it.isNotEmpty() }?.let { result.put(name, it) }
            }
            retriever.embeddedPicture?.let { bytes ->
                val options = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)
                if (options.outWidth > 0 && options.outHeight > 0) {
                    options.inJustDecodeBounds = false
                    options.inSampleSize = 1
                    while (maxOf(options.outWidth, options.outHeight) / options.inSampleSize > 1024) {
                        options.inSampleSize *= 2
                    }
                    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)?.let { bitmap ->
                        try {
                            val hash = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
                            val directory = File(context.cacheDir, "covers").apply { mkdirs() }
                            val cover = File(directory, "$hash.jpg")
                            if (!cover.isFile) cover.outputStream().use { bitmap.compress(Bitmap.CompressFormat.JPEG, 90, it) }
                            result.put("coverUri", Uri.fromFile(cover).toString())
                        } finally { bitmap.recycle() }
                    }
                }
            }
        } catch (_: Exception) {
            // Missing or malformed optional tags must not prevent audio playback.
        } finally {
            retriever.release()
        }
        return result
    }
}
