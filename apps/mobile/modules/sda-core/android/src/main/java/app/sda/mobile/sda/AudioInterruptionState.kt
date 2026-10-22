package app.sda.mobile.sda

/** Playback intent across focus changes; this never owns/rewinds a decoder. */
class AudioInterruptionState {
    var held = false; private set
    var resumeOnGain = false; private set
    var pauseRequested = false; private set
    var permanentLoss = false; private set
    fun granted() { held = true; permanentLoss = false; pauseRequested = false }
    fun lost(temporary: Boolean, wasPlaying: Boolean) {
        held = false
        resumeOnGain = temporary && (wasPlaying || resumeOnGain) && !permanentLoss
        permanentLoss = !temporary || permanentLoss
        pauseRequested = wasPlaying || pauseRequested
    }
    fun gained(): Boolean {
        if (permanentLoss) return false
        held = true; pauseRequested = false
        return resumeOnGain
    }
    fun resumed() { resumeOnGain = false; pauseRequested = false }
}
