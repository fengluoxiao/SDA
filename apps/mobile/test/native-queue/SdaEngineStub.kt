package com.sda.nativebridge

/** JVM harness only; this file is outside the Android production source sets. */
object SdaEngine {
    var created = 0; var av3aCreated = 0; var closed = 0; var steps = 0
    var onStep: (() -> Unit)? = null
    fun reset() { created = 0; av3aCreated = 0; closed = 0; steps = 0; onStep = null }
    fun nativeStereoReferenceCreate(path: String, alac: Boolean): Long { created++; return 1L }
    fun nativeAv3aReferenceCreate(path: String): Long { created++; av3aCreated++; return 1L }
    fun nativeAv3aReferenceCreateWithHrtf(path:String,hrtf:String):Long { check(java.io.File(hrtf).isFile);return nativeAv3aReferenceCreate(path) }
    fun nativeStereoReferenceStep(handle: Long): String { steps++; onStep?.invoke(); return """{"done":true,"measurement":{"source_lufs":-6.0}}""" }
    fun nativeStereoReferenceClose(handle: Long) { closed++ }
}
