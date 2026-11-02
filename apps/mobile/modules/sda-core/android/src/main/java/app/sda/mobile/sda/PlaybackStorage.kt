package app.sda.mobile.sda

import java.io.File
import java.nio.file.Files

/** Deletes only disposable app-owned copies. Never traverses a symlink or the library. */
internal object PlaybackStorage {
    private var swept = false
    private val scratch = Regex("sda-format-[0-9]+-[0-9]+|sda-360ra-(?:input-)?[0-9]+-.+\\.(?:mp4|mhas)|sda-seekable-.+|sda-vivid-reference-[0-9]+-[0-9]+\\.mhas")

    @Synchronized fun startup(cache: File, protected: Set<File>, library: File? = null) {
        if (swept) return
        // Called before any decoder/import is created, once per process (not JS/module reload).
        swept = true
        clean(cache, protected, cleanPicker = true)
        // A killed atomic import can leave a staging file, never a complete song.
        library?.listFiles()?.filter { it.name.matches(Regex("pending-[0-9]+\\.audio")) && !Files.isSymbolicLink(it.toPath()) && it.isFile }
            ?.forEach { it.delete() }
    }
    fun cleanBuiltins(directory: File, ids: Set<String>) {
        if (Files.isSymbolicLink(directory.toPath())) return
        directory.listFiles()?.filter { file ->
            !Files.isSymbolicLink(file.toPath()) && file.isFile &&
            file.name.matches(Regex("[0-9a-f]{64}\\.(json|tmp)")) &&
            (file.extension == "tmp" || file.nameWithoutExtension !in ids)
        }?.forEach { it.delete() }
    }
    fun clean(cache: File, protected: Set<File>, cleanPicker: Boolean) {
        val base = cache.canonicalFile
        val keep = protected.mapNotNull { runCatching { it.canonicalFile.toPath() }.getOrNull() }
        fun remove(candidate: File) {
            val path = candidate.toPath()
            if (Files.isSymbolicLink(path)) return
            val actual = runCatching { candidate.canonicalFile.toPath() }.getOrNull() ?: return
            if (!actual.startsWith(base.toPath()) || actual == base.toPath() || keep.any { it == actual || it.startsWith(actual) }) return
            if (candidate.isDirectory) candidate.listFiles()?.forEach { remove(it) }
            candidate.delete() // A protected child leaves its parent nonempty.
        }
        base.listFiles()?.filter { scratch.matches(it.name) }?.forEach { remove(it) }
        if (cleanPicker) File(base, "DocumentPicker").takeUnless { Files.isSymbolicLink(it.toPath()) }
            ?.listFiles()?.forEach { remove(it) }
        val covers = File(base, "covers")
        if (Files.isSymbolicLink(covers.toPath())) return
        val images = covers.listFiles()?.filter { it.isFile && it.name.matches(Regex("[0-9a-f]{64}\\.jpg")) }
            ?.sortedByDescending { it.lastModified() }.orEmpty()
        var bytes = 0L
        for (image in images) {
            bytes += image.length()
            if (bytes > 64L * 1024 * 1024) remove(image)
        }
    }
}
