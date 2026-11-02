package app.sda.mobile.sda

import com.sda.nativebridge.SdaEngine
import org.json.JSONObject
import java.io.File

/** Current loaded-source programme settings; no PCM or cross-launch results. */
internal class PreparedStereoReference {
    private data class Source(val path: String, val bytes: Long, val modified: Long)
    private data class Entry(val source: Source, val renderer:Source?, val measurement: String)
    private val entries = mutableMapOf<String, Entry>()
    private fun source(file: File) = Source(file.canonicalPath, file.length(), file.lastModified())
    @Synchronized fun forSource(file: File, hash: String, referenceHrtf:String?=null): String? = entries[hash]?.takeIf { it.source == source(file) && it.renderer == referenceHrtf?.let { path -> source(File(path)) } }?.measurement
    @Synchronized fun clear() = entries.clear()
    @Synchronized fun retain(hashes: Set<String>) { entries.keys.retainAll(hashes) }
    fun prepare(file: File, hash: String, alac: Boolean, av3a: Boolean = false, keepPending: Boolean = true, referenceHrtf:String?=null, cancelled: () -> Boolean): String {
        val identity = source(file)
        val renderer=referenceHrtf?.let {source(File(it))}
        forSource(file, hash, referenceHrtf)?.let { check(!cancelled()) { "立体声分析已取消" }; return it }
        check(!cancelled()) { "立体声分析已取消" }
        val reference = if (av3a && referenceHrtf != null) SdaEngine.nativeAv3aReferenceCreateWithHrtf(file.absolutePath, referenceHrtf) else if (av3a) SdaEngine.nativeAv3aReferenceCreate(file.absolutePath) else SdaEngine.nativeStereoReferenceCreate(file.absolutePath, alac)
        check(reference != 0L) { "立体声响度分析初始化失败" }
        try {
            while (true) {
                check(!cancelled()) { "立体声分析已取消" }
                val step = JSONObject(SdaEngine.nativeStereoReferenceStep(reference))
                if (!step.getBoolean("done")) continue
                val measurement = step.getJSONObject("measurement").toString()
                check(identity == source(file) && renderer == referenceHrtf?.let {source(File(it))}) { "分析时音源发生变化，请重新打开文件" }
                check(!cancelled()) { "立体声分析已取消" }
                if (keepPending) synchronized(this) { entries[hash] = Entry(identity, renderer, measurement) }
                return measurement
            }
        } finally { SdaEngine.nativeStereoReferenceClose(reference) }
    }
}
