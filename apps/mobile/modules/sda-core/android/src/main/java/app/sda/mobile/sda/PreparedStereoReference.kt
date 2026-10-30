package app.sda.mobile.sda

import com.sda.nativebridge.SdaEngine
import org.json.JSONObject
import java.io.File

/** Import-owned measurements only. Never serialize or read a loudness disk cache. */
internal class PreparedStereoReference {
    private data class Source(val path: String, val bytes: Long, val modified: Long)
    private data class Entry(val source: Source, val measurement: String)
    private val entries = mutableMapOf<String, Entry>()
    private fun source(file: File) = Source(file.canonicalPath, file.length(), file.lastModified())
    @Synchronized fun ready(file: File, hash: String): String? = entries[hash]?.takeIf { it.source == source(file) }?.measurement
    @Synchronized fun clear() = entries.clear()
    fun prepare(file: File, hash: String, alac: Boolean, cancelled: () -> Boolean): String {
        val identity = source(file)
        check(!cancelled()) { "立体声分析已取消" }
        val reference = SdaEngine.nativeStereoReferenceCreate(file.absolutePath, alac)
        check(reference != 0L) { "立体声响度分析初始化失败" }
        try {
            while (true) {
                check(!cancelled()) { "立体声分析已取消" }
                val step = JSONObject(SdaEngine.nativeStereoReferenceStep(reference))
                if (!step.getBoolean("done")) continue
                val measurement = step.getJSONObject("measurement").toString()
                check(identity == source(file)) { "分析时音源发生变化，请重新打开文件" }
                synchronized(this) { entries[hash] = Entry(identity, measurement) }
                return measurement
            }
        } finally { SdaEngine.nativeStereoReferenceClose(reference) }
    }
}
