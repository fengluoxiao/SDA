package app.sda.mobile.sda

import com.sda.nativebridge.SdaEngine
import org.json.JSONObject
import java.io.File

/** One-time handoff of a freshly computed import measurement to its first playback. */
internal class PreparedStereoReference {
    private data class Source(val path: String, val bytes: Long, val modified: Long)
    private data class Entry(val source: Source, val measurement: String)
    private val entries = mutableMapOf<String, Entry>()
    private fun source(file: File) = Source(file.canonicalPath, file.length(), file.lastModified())
    @Synchronized fun take(file: File, hash: String): String? = entries.remove(hash)?.takeIf { it.source == source(file) }?.measurement
    @Synchronized fun clear() = entries.clear()
    fun prepare(file: File, hash: String, alac: Boolean, av3a: Boolean = false, keepPending: Boolean = true, cancelled: () -> Boolean): String {
        val identity = source(file)
        check(!cancelled()) { "立体声分析已取消" }
        val reference = if (av3a) SdaEngine.nativeAv3aReferenceCreate(file.absolutePath) else SdaEngine.nativeStereoReferenceCreate(file.absolutePath, alac)
        check(reference != 0L) { "立体声响度分析初始化失败" }
        try {
            while (true) {
                check(!cancelled()) { "立体声分析已取消" }
                val step = JSONObject(SdaEngine.nativeStereoReferenceStep(reference))
                if (!step.getBoolean("done")) continue
                val measurement = step.getJSONObject("measurement").toString()
                check(identity == source(file)) { "分析时音源发生变化，请重新打开文件" }
                if (keepPending) synchronized(this) { entries[hash] = Entry(identity, measurement) }
                return measurement
            }
        } finally { SdaEngine.nativeStereoReferenceClose(reference) }
    }
}
