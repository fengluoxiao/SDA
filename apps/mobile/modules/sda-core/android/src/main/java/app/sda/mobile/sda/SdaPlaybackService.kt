package app.sda.mobile.sda

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import android.util.Log
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicLong

/** A playback lifetime, not an audio renderer. All native PCM stays in the
 * lock-free AAudio path; this service never decodes or holds the engine lock. */
object PlaybackKeepAlive {
    data class State(val token: Long, val title: String, val playing: Boolean)
    private val serial = AtomicLong()
    private val lock = Any()
    @Volatile private var state: State? = null
    @Volatile var serviceRunning = false
    @Volatile var wakeLockHeld = false
    @Volatile private var failure = ""
    @Volatile private var application: Context? = null
    fun owns(token: Long): Boolean = token != 0L && state?.token == token
    fun end(token: Long): Boolean = application?.let { end(it, token) } ?: false
    internal fun snapshot(): State? = state

    fun begin(context: Context, title: String): Long = synchronized(lock) {
        application = context.applicationContext
        val token = serial.incrementAndGet()
        state = State(token, title.take(200), true)
        failure = ""
        try {
            context.applicationContext.startForegroundService(Intent(context, SdaPlaybackService::class.java))
        } catch (error: RuntimeException) {
            failure = error.javaClass.simpleName
            Log.w("SdaPlayback", "Cannot start background playback lease: $failure")
        }
        token
    }
    fun setPlaying(token: Long, playing: Boolean): Boolean = synchronized(lock) {
        val current = state ?: return@synchronized false
        if (current.token != token) return@synchronized false
        state = current.copy(playing = playing)
        SdaPlaybackService.refresh()
        true
    }
    fun finish(context: Context, token: Long) {
        if (!setPlaying(token, false)) return
        // Let background playlist advancement hand over the existing media
        // service. A stale EOF may never stop a newly started track.
        Handler(Looper.getMainLooper()).postDelayed({ end(context, token) }, 5_000)
    }
    fun end(context: Context, token: Long): Boolean = synchronized(lock) {
        if (state?.token != token) return@synchronized false
        state = null
        context.applicationContext.stopService(Intent(context, SdaPlaybackService::class.java))
        true
    }
    fun diagnostics(): JSONObject = JSONObject()
        .put("active", state != null).put("serviceRunning", serviceRunning)
        .put("partialWakeLockHeld", wakeLockHeld).put("playing", state?.playing == true)
        .put("startFailureCategory", failure)
}

class SdaPlaybackService : Service() {
    private val handler = Handler(Looper.getMainLooper())
    private var wakeLock: PowerManager.WakeLock? = null
    private val renew = Runnable { applyState() }
    companion object {
        private const val CHANNEL = "sda_playback"
        private const val NOTIFICATION = 48001
        @Volatile private var instance: SdaPlaybackService? = null
        internal fun refresh() {
            instance?.let { service -> service.handler.post { service.applyState() } }
        }
    }
    override fun onCreate() {
        super.onCreate()
        instance = this
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(NotificationChannel(CHANNEL, "音乐播放", NotificationManager.IMPORTANCE_LOW).apply {
            description = "SDA 后台音频播放状态"
            setSound(null, null)
            enableVibration(false)
        })
        wakeLock = getSystemService(PowerManager::class.java).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "SDA:AudioPlayback").apply {
            setReferenceCounted(false)
        }
        PlaybackKeepAlive.serviceRunning = true
    }
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        applyState()
        return START_NOT_STICKY
    }
    private fun notification(state: PlaybackKeepAlive.State): Notification {
        val builder = Notification.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle(state.title.ifBlank { "Spatial Decoder Atelier" })
            .setContentText(if (state.playing) "空间音频播放中" else "播放已暂停")
            .setCategory(Notification.CATEGORY_TRANSPORT)
            .setOnlyAlertOnce(true).setOngoing(true).setShowWhen(false)
        packageManager.getLaunchIntentForPackage(packageName)?.let { launch ->
            builder.setContentIntent(PendingIntent.getActivity(this, 0, launch,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
        }
        if (Build.VERSION.SDK_INT >= 31) builder.setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE)
        return builder.build()
    }
    private fun applyState() {
        handler.removeCallbacks(renew)
        val state = PlaybackKeepAlive.snapshot()
        if (state == null) { stopSelf(); return }
        val notification = notification(state)
        if (Build.VERSION.SDK_INT >= 29) startForeground(NOTIFICATION, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
        else startForeground(NOTIFICATION, notification)
        val lock = wakeLock
        if (state.playing) {
            // A bounded, renewed CPU lease; screen and UI rendering stay asleep.
            // It is released on pause, EOF, error, stop and module destruction.
            lock?.acquire(10 * 60 * 1000L)
            handler.postDelayed(renew, 5 * 60 * 1000L)
        } else if (lock?.isHeld == true) lock.release()
        PlaybackKeepAlive.wakeLockHeld = lock?.isHeld == true
    }
    override fun onDestroy() {
        handler.removeCallbacks(renew)
        wakeLock?.let { if (it.isHeld) it.release() }
        PlaybackKeepAlive.wakeLockHeld = false
        PlaybackKeepAlive.serviceRunning = false
        if (instance === this) instance = null
        stopForeground(STOP_FOREGROUND_REMOVE)
        super.onDestroy()
    }
    override fun onBind(intent: Intent?): IBinder? = null
}
