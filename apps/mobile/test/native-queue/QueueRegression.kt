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
    refs.prepare(source,"hash",true){false};check(refs.take(source,"hash") != null)
    repeat(10) { check(refs.take(source,"hash") == null) }
    check(SdaEngine.created == 1 && SdaEngine.closed == 1)
    refs.prepare(source,"hash",true){false}
    source.writeText("different-file");check(refs.take(source,"hash") == null)
    refs.prepare(source,"hash",true){false};check(SdaEngine.created == 3 && SdaEngine.closed == 3)
    check(source.setLastModified(source.lastModified()+5000));check(refs.take(source,"hash") == null)
    refs.prepare(source,"live",true,keepPending=false){false};check(refs.take(source,"live") == null)
    SdaEngine.reset();var checks=0
    check(runCatching { refs.prepare(source,"cancel",false){++checks > 1} }.isFailure)
    check(SdaEngine.created == 1 && SdaEngine.closed == 1 && refs.take(source,"cancel") == null)
    SdaEngine.reset();SdaEngine.onStep={source.appendText("changed during analysis")}
    check(runCatching { refs.prepare(source,"mutated",false){false} }.isFailure)
    check(SdaEngine.closed == 1 && refs.take(source,"mutated") == null)
    SdaEngine.reset()
    refs.prepare(source,"av3a",false,true){false}
    check(SdaEngine.av3aCreated == 1 && SdaEngine.closed == 1)
    check(refs.take(source,"av3a") != null)
    refs.clear();check(refs.take(source,"hash") == null)
    println("PASS import reference: one-time fresh handoff, no repeat reuse, hash/path/file-change guard, cancellation, mid-scan mutation, handle cleanup")
}
