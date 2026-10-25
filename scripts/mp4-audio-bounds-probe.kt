package app.sda.mobile.sda
import java.io.File
import java.io.IOException

fun main(args: Array<String>) {
    val root = File(args[0]); val expected = File(root, "expected.eac3").length()
    val accepted = listOf("cursor-zero-ctts.m4a", "cursor-short-zero-ctts.m4a", "cursor-nonzero-ctts.m4a", "cursor32.m4a", "cursor64.m4a", "cursor-clipped-tail.m4a", "cursor-id3-tail.m4a", "cursor-short-padding.m4a", "cursor-invalid-free-tail.m4a", "cursor-short-extended-tail.m4a", "cursor-overdeclared-mdat.m4a", "cursor-overdeclared-mdat64.m4a")
    for (name in accepted) {
        val result = checkNotNull(Mp4AudioBounds.validateEac3(File(root, name))) { name }
        check(result.samples == 9270 && result.referencedBytes == expected) { "Lost audio samples: $name" }
    }
    val rejected = listOf("truncated-zero-ctts.m4a", "truncated-nonzero-ctts.m4a", "truncated.m4a", "truncated-52151.m4a", "truncated-mdat64.m4a", "invalid-first-box.m4a", "truncated-moov.m4a")
    for (name in rejected) {
        var error: IOException? = null
        try { Mp4AudioBounds.validateEac3(File(root, name)) } catch (e: IOException) { error = e }
        check(error != null) { "Truncated data was accepted: $name" }
        if (name in listOf("truncated-52151.m4a", "truncated-zero-ctts.m4a", "truncated-nonzero-ctts.m4a")) check(error.message?.contains("52151") == true) { "Wrong missing byte count" }
    }
    if (args.size > 1) {
        var message = ""
        try { Mp4AudioBounds.validateEac3(File(args[1])) } catch (e: IOException) { message = e.message ?: "" }
        check(message.contains("52151")) { "Original truncated source was not identified" }
        println("Original input: missing 52151 bytes confirmed; source opened read-only")
    }
    println("PASS: 12 complete-audio containers accepted; 7 truncated/malformed containers rejected; exact missing-byte count verified")
}
