package app.sda.mobile.sda

import com.sda.nativebridge.SdaEngine
import java.io.File
fun main(args: Array<String>) {
    val a = NativeQueueTrack("a", "file:///a.mhas", "a.mhas", "{}")
    val b = NativeQueueTrack("b", "file:///b.mp3", "b.mp3", "{}")
    val queue = NativePlaybackQueue()
    queue.configure(listOf(a,b), "sequence")
    check(queue.next("a",true) == b); check(queue.next("b",true) == null)
    check(queue.next("missing",true) == null)
    queue.configure(listOf(a,b), "repeat-all"); check(queue.next("b",true) == a)
    queue.configure(listOf(a,b), "repeat-one"); check(queue.next("a",true) == a)
    check(queue.next("a",false,1) == b); check(queue.next("a",false,-1) == b)
    queue.configure(listOf(a),"repeat-all"); check(queue.next("a",true) == a)
    queue.configure(emptyList(),"sequence"); check(queue.next("a",true) == null)
    check(runCatching { queue.configure(listOf(a,a),"sequence") }.isFailure)
    check(runCatching { queue.configure(listOf(a),"unknown") }.isFailure)
    println("PASS native queue: sequence, EOF, both repeat modes, manual skip, removed source, clear, duplicate validation")
    val source=File(args[0], "reference-fixture.bin").apply { writeText("one") }
    val refs=PreparedStereoReference();SdaEngine.reset()
    refs.prepare(source,"hash",true){false};check(refs.ready(source,"hash") != null)
    repeat(10) { check(refs.ready(source,"hash") != null) }
    check(SdaEngine.created == 1 && SdaEngine.closed == 1)
    check(refs.ready(source,"other-hash") == null)
    source.writeText("different-file");check(refs.ready(source,"hash") == null)
    refs.prepare(source,"hash",true){false};check(SdaEngine.created == 2 && SdaEngine.closed == 2)
    check(source.setLastModified(source.lastModified()+5000));check(refs.ready(source,"hash") == null)
    SdaEngine.reset();var checks=0
    check(runCatching { refs.prepare(source,"cancel",false){++checks > 1} }.isFailure)
    check(SdaEngine.created == 1 && SdaEngine.closed == 1 && refs.ready(source,"cancel") == null)
    SdaEngine.reset();SdaEngine.onStep={source.appendText("changed during analysis")}
    check(runCatching { refs.prepare(source,"mutated",false){false} }.isFailure)
    check(SdaEngine.closed == 1 && refs.ready(source,"mutated") == null)
    refs.clear();check(refs.ready(source,"hash") == null)
    println("PASS import reference: one fresh scan, repeat reuse, hash/path/file-change guard, cancellation, mid-scan mutation, handle cleanup")
}
