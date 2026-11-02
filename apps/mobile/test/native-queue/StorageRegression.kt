package app.sda.mobile.sda

import java.io.File
import java.nio.file.Files

fun main(args: Array<String>) {
    val base = File(args[0], "cache").apply { mkdirs() }
    val library = File(args[0], "playback-library").apply { mkdirs() }
    val song = File(library, "user.m4a").apply { writeText("original") }
    val picker = File(base, "DocumentPicker").apply { mkdirs() }
    val queued = File(picker, "queued.m4a").apply { writeText("playing") }
    val obsolete = File(picker, "orphan.m4a").apply { writeText("duplicate") }
    val resource = File(base, "sda-format-123-4").apply { mkdirs() }
    File(resource, "hrtf.f32").writeText("extracted")
    val active = File(base, "sda-format-123-5").apply { mkdirs() }
    val activeAsset = File(active, "room.json").apply { writeText("active") }
    val scratch = File(base, "sda-seekable-123.m4a").apply { writeText("temporary") }
    val unrelated = File(base, "sda-format-not-a-resource").apply { mkdirs() }
    File(unrelated, "keep").writeText("unrelated")
    val covers = File(base, "covers").apply { mkdirs() }
    val image = File(covers, "a".repeat(64)+".jpg").apply { writeText("queued cover") }
    // A large stale cover may be removed; referenced artwork remains even above quota.
    val stale = File(covers, "b".repeat(64)+".jpg").apply { java.io.RandomAccessFile(this,"rw").use { it.setLength(65L*1024*1024) }; setLastModified(1) }
    PlaybackStorage.clean(base, setOf(queued,activeAsset,image),true)
    check(song.readText()=="original" && queued.readText()=="playing" && activeAsset.exists() && image.exists())
    check(!obsolete.exists() && !resource.exists() && !scratch.exists() && !stale.exists())
    check(unrelated.isDirectory)
    val afterStartup = File(base,"sda-seekable-live.m4a").apply { writeText("live decoder") }
    PlaybackStorage.startup(base, setOf(afterStartup,queued))
    val liveImport=File(picker,"import-in-progress.m4a").apply { writeText("live import") }
    PlaybackStorage.startup(base,emptySet())
    check(afterStartup.exists() && liveImport.exists()) // Reload must not sweep live files.
    // The test also runs on Unix CI, where symlinks do not require Windows privilege.
    val external=File(args[0],"external").apply { mkdirs() }
    val outsideSong=File(external,"original.wav").apply { writeText("source") }
    val link=File(base,"sda-format-999-9")
    if (runCatching { Files.createSymbolicLink(link.toPath(),external.toPath()) }.isSuccess) {
        PlaybackStorage.clean(base,emptySet(),true)
        check(outsideSong.readText()=="source" && Files.isSymbolicLink(link.toPath()))
        Files.delete(link.toPath())
    }
    val rooms=File(args[0],"builtin-rooms").apply { mkdirs() }
    val current=File(rooms,"c".repeat(64)+".json").apply { writeText("current room") }
    val previous=File(rooms,"d".repeat(64)+".json").apply { writeText("previous bundled response") }
    val custom=File(rooms,"custom-room.json").apply { writeText("user data") }
    PlaybackStorage.cleanBuiltins(rooms,setOf("c".repeat(64)))
    check(current.exists() && custom.exists() && !previous.exists())
    println("Playback storage: queue/library protection, abandoned copies/resources, cover quota, process lifetime and symlink safety passed")
}
