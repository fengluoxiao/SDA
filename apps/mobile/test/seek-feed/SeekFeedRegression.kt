package app.sda.mobile.sda

import org.json.JSONObject

// The runner inserts the actual SdaModule feeder expression here, including any
// JSON telemetry reads. No separately maintained copy of the decision is tested.
private fun shouldWait(paused: Boolean, state: JSONObject): Boolean = __PRODUCTION_GATE__

private fun status(rendererPaused: Boolean, recovering: Boolean = false) = JSONObject()
    .put("paused", rendererPaused).put("preparingAudio", false)
    .put("outputDevice", JSONObject().put("recovering", recovering))

fun main() {
    // Ordinary running playback, pre-start telemetry, and post-SeekReset telemetry
    // have the same feed intent. Balance OFF / already measured sources also
    // report preparingAudio=false; that flag cannot distinguish seek startup.
    for (rendererPaused in listOf(false, true)) {
        check(!shouldWait(false, status(rendererPaused))) { "Renderer startup telemetry blocked the decoder" }
        check(shouldWait(true, status(rendererPaused))) { "User/focus pause must keep the feeder stopped" }
        check(shouldWait(false, status(rendererPaused, true))) { "Output recovery must retain its bounded reserve" }
    }
    check(!shouldWait(false, JSONObject().put("paused", true))) { "A missing outputDevice must not block seeking" }

    // Model the asynchronous native contract: SeekReset -> renderer paused,
    // feed 4k blocks -> bounded preroll/StartAt -> output clock advances.
    // The old gate never reaches the first pull, even after pressing resume.
    var decoded = 0L
    var consumed = 0L
    var paused = false
    var native = status(false)
    fun seek(target: Long) { decoded = target - 6144; consumed = target; native = status(true) }
    fun tick() {
        if (shouldWait(paused, native)) return
        decoded += 4096
        if (decoded >= consumed + 2048) native.put("paused", false)
        if (!native.getBoolean("paused")) consumed += 512
    }
    seek(960000)
    repeat(8) { tick() }
    check(consumed > 960000) { "Playing seek never resumed output" }
    paused = true
    seek(480000)
    val heldDecoded = decoded
    repeat(8) { tick() }
    check(consumed == 480000L && decoded == heldDecoded) { "Paused seek resumed without user intent" }
    paused = false
    repeat(8) { tick() }
    check(consumed > 480000) { "Resume after paused seek was blocked by native pause telemetry" }
    // Several clicks before native work runs: preserve the final target and
    // exercise pause/resume changes both before and after reset is visible.
    repeat(100) { i ->
        seek(240000 + i * 48000L)
        paused = i % 2 == 0
        tick()
    }
    val finalTarget = consumed
    paused = false
    repeat(8) { tick() }
    check(consumed > finalTarget) { "Repeated seeks left the feeder stopped" }
    native.getJSONObject("outputDevice").put("recovering", true)
    val recoveryPosition = consumed
    repeat(8) { tick() }
    check(consumed == recoveryPosition)
    native.getJSONObject("outputDevice").put("recovering", false)
    tick()
    check(consumed > recoveryPosition)
    println("Android production feed gate: playing/paused seek, resume, 100 repeated seeks and output recovery PASS")
}
