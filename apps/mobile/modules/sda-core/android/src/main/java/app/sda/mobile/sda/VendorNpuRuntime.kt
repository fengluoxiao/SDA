package app.sda.mobile.sda

import android.content.Context
import android.os.Build
import android.system.Os
import android.util.Log
import com.sda.nativebridge.SdaEngine
import org.json.JSONObject
import java.io.File
import java.security.MessageDigest
import java.util.concurrent.atomic.AtomicBoolean

/** Package-owned optional runtimes. No network, PCM/model cache or UI-thread work. */
internal object VendorNpuRuntime {
    private val started = AtomicBoolean(false)
    // Hexagon NN's public SoC table maps these chips to v60, before QNN HTP.
    private val v60SocIds = setOf(246, 291, 292, 305, 310, 311, 312, 315, 316, 317, 318, 319, 324, 325, 326, 327)
    private fun digest(file: File): String {
        val hash = MessageDigest.getInstance("SHA-256")
        file.inputStream().use { input ->
            val bytes = ByteArray(64 * 1024)
            while (true) { val n = input.read(bytes); if (n < 0) break; hash.update(bytes, 0, n) }
        }
        return hash.digest().joinToString("") { "%02x".format(it) }
    }
    fun prepare(context: Context) {
        if (!started.compareAndSet(false, true)) return
        val app = context.applicationContext
        Thread({
            android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_BACKGROUND)
            var directory = ""
            var preparation = ""
            try {
                check(Build.SUPPORTED_ABIS.firstOrNull() == "arm64-v8a") { "vendor-runtimes-require-arm64; retaining-native-or-NNAPI" }
                val manifest = JSONObject(app.assets.open("sda-npu/manifest.json").bufferedReader().use { it.readText() })
                check(manifest.getString("version") == "2.3.0") { "runtime-manifest-version-mismatch" }
                val base = File(app.codeCacheDir, "sda-audio-runtime").apply { check(mkdirs() || isDirectory) }
                val version = File(base, "runtime-2.3.0").apply { check(mkdirs() || isDirectory) }
                val soc = listOf("/sys/devices/soc0/soc_id", "/sys/devices/system/soc/soc0/id").firstNotNullOfOrNull {
                    runCatching { File(it).readText().trim().toIntOrNull() }.getOrNull()
                }
                val oldDsp = soc in v60SocIds
                val files = manifest.getJSONObject("files")
                for (name in files.keys()) {
                    check(name.matches(Regex("(common|legacy)/lib[A-Za-z0-9_.]+\\.so"))) { "invalid-runtime-asset-name" }
                    if (name.startsWith("common/") && (Build.VERSION.SDK_INT < 31 || oldDsp)) continue
                    val target = File(version, name)
                    check(target.canonicalPath.startsWith(version.canonicalPath + File.separator))
                    val entry = files.getJSONObject(name)
                    val hash = entry.getString("sha256")
                    if (target.isFile && target.length() == entry.getLong("size") && digest(target) == hash) continue
                    check(target.parentFile!!.mkdirs() || target.parentFile!!.isDirectory)
                    val temporary = File(target.parentFile, target.name + ".part")
                    check(!temporary.exists() || temporary.delete())
                    app.assets.open("sda-npu/$name").use { input -> temporary.outputStream().use { output -> input.copyTo(output, 64 * 1024) } }
                    check(temporary.length() == entry.getLong("size") && digest(temporary) == hash) { "runtime-integrity-failed:$name" }
                    check(temporary.setReadOnly())
                    Os.rename(temporary.absolutePath, target.absolutePath)
                }
                // Match the public Hexagon API's FastRPC loader search convention.
                val legacy = File(version, "legacy").absolutePath
                val current = Os.getenv("ADSP_LIBRARY_PATH").orEmpty()
                val search = listOf(legacy, File(version, "common").absolutePath, current, "/vendor/lib/rfsa/adsp", "/vendor/dsp", "/dsp").filter { it.isNotBlank() }.joinToString(";")
                Os.setenv("ADSP_LIBRARY_PATH", search, true)
                directory = version.absolutePath
                if (oldDsp) preparation = "legacy-Hexagon-v60-selected; modern-HTP-runtimes-skipped; socId=$soc"
                // Keep exactly one runtime version. No track-specific files live here.
                base.listFiles()?.filter { it.isDirectory && it.name.startsWith("runtime-") && it != version && it.canonicalFile.parentFile == base.canonicalFile }?.forEach { it.deleteRecursively() }
            } catch (error: Exception) {
                preparation = "optional-vendor-runtime-preparation:${error.message}"
            }
            val error = SdaEngine.nativeConfigureAcceleration(directory, Build.VERSION.SDK_INT, preparation)
            if (error.isNotEmpty()) Log.w("SdaNpu", error)
        }, "sda-runtime-prepare").start()
    }
}
