package app.sda.mobile.sda

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import org.json.JSONObject

/** One public audio-focus request per playback lease, independent of native PCM. */
internal object PlaybackAudioFocus {
    private data class Request(val token: Long, val id: Long, val request: AudioFocusRequest,
        val state: AudioInterruptionState = AudioInterruptionState())
    private val lock = Any()
    private var serial = 0L
    private var manager: AudioManager? = null
    private var current: Request? = null
    private var token = 0L
    private var reason = "idle"
    fun begin(context: Context, lease: Long) = synchronized(lock) {
        abandonLocked()
        manager = context.applicationContext.getSystemService(AudioManager::class.java)
        token = lease; reason = "preparing"
    }
    private fun abandonLocked() {
        val old = current
        current = null // Invalidate old callbacks before asking AudioManager to abandon.
        if (old != null) manager?.abandonAudioFocusRequest(old.request)
    }
    fun request(lease: Long): Boolean = synchronized(lock) {
        if (lease != token || !PlaybackKeepAlive.owns(lease)) return@synchronized false
        current?.takeIf { it.state.held && !it.state.permanentLoss }?.let { return@synchronized true }
        abandonLocked()
        val id = ++serial
        val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
            .setWillPauseWhenDucked(true).setAcceptsDelayedFocusGain(false)
            .setOnAudioFocusChangeListener({ change -> changed(lease, id, change) }, Handler(Looper.getMainLooper())).build()
        val next = Request(lease, id, request)
        current = next
        val granted = manager?.requestAudioFocus(request) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
        if (granted) next.state.granted()
        reason = if (granted) "held" else "request-denied"
        granted
    }
    fun userPause(lease: Long) = synchronized(lock) {
        if (lease == token) { abandonLocked(); reason = "user-paused" }
    }
    fun end(lease: Long) = synchronized(lock) {
        if (lease == token) { abandonLocked(); token = 0L; reason = "idle" }
    }
    private fun changed(lease: Long, id: Long, change: Int) {
        val action = synchronized(lock) {
            val own = current?.takeIf { it.token == lease && it.id == id && PlaybackKeepAlive.owns(lease) }
                ?: return
            when (change) {
                AudioManager.AUDIOFOCUS_GAIN -> {
                    val resume = own.state.gained()
                    reason = if (own.state.held) "held" else "permanent-loss"
                    if (resume) true else null
                }
                AudioManager.AUDIOFOCUS_LOSS, AudioManager.AUDIOFOCUS_LOSS_TRANSIENT,
                AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> {
                    val transient = change != AudioManager.AUDIOFOCUS_LOSS
                    own.state.lost(transient, PlaybackKeepAlive.snapshot()?.playing == true)
                    reason = if (transient) "transient-loss" else "permanent-loss"
                    if (own.state.pauseRequested) false else null
                }
                else -> null
            }
        }
        if (action != null) SdaPlaybackService.focusChanged(lease, id, action)
    }
    fun currentRequest(lease: Long, id: Long): Boolean = synchronized(lock) {
        current?.let { it.token == lease && it.id == id } == true
    }
    fun shouldApply(lease: Long, playing: Boolean): Boolean = synchronized(lock) {
        val own = current?.takeIf { it.token == lease } ?: return@synchronized false
        if (playing) own.state.held && own.state.resumeOnGain && !own.state.permanentLoss
        else own.state.pauseRequested && !own.state.held
    }
    fun didResume(lease: Long) = synchronized(lock) {
        current?.takeIf { it.token == lease }?.state?.resumed()
    }
    fun diagnostics(): JSONObject = synchronized(lock) {
        JSONObject().put("state", reason).put("held", current?.state?.held == true)
            .put("resumeOnGain", current?.state?.resumeOnGain == true)
            .put("permanentLoss", current?.state?.permanentLoss == true)
    }
}
