package app.sda.mobile.sda

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.IntentFilter
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.media.MediaMetadata as AndroidMediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import android.os.SystemClock
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.atomic.AtomicLong

/** System media surface and playback lifetime; native PCM stays on AAudio. */
object PlaybackKeepAlive {
    data class Progress(val positionMs: Long, val preparing: Boolean = false)
    interface Controls {
        fun setPlaying(playing: Boolean): Boolean
        fun setInterrupted(playing: Boolean): Boolean = setPlaying(playing)
        fun stop()
        fun skip(direction: Int)
        fun progress(): Progress?
    }
    data class State(val token: Long, val title: String, val playing: Boolean,
        val hash: String = "", val artist: String = "", val album: String = "",
        val durationMs: Long = 0, val coverUri: String = "", val controls: Controls? = null)
    private val serial = AtomicLong()
    private val lock = Any()
    @Volatile private var state: State? = null
    @Volatile private var queue: Map<String, JSONObject> = emptyMap()
    @Volatile var serviceRunning = false
    @Volatile var wakeLockHeld = false
    @Volatile var mediaSessionActive = false
    @Volatile private var failure = ""
    @Volatile private var application: Context? = null
    fun owns(token: Long): Boolean = token != 0L && state?.token == token
    fun end(token: Long): Boolean = application?.let { end(it, token) } ?: false
    internal fun snapshot(): State? = state
    internal fun canSkip(): Boolean = queue.size > 1

    fun configureQueue(text: String) {
        val rows = JSONArray(text)
        val next = mutableMapOf<String, JSONObject>()
        for (i in 0 until rows.length()) {
            val row = rows.getJSONObject(i)
            next[row.getString("hash")] = row.optJSONObject("metadata") ?: JSONObject()
        }
        queue = next
        SdaPlaybackService.refresh()
    }
    private fun metadata(current: State, data: JSONObject): State = current.copy(
        title = data.optString("title").takeIf { it.isNotBlank() }?.take(200) ?: current.title,
        artist = data.optString("artist").take(500), album = data.optString("album").take(500),
        durationMs = data.optLong("durationMs").coerceAtLeast(0), coverUri = data.optString("coverUri"))
    fun setMetadata(hash: String, text: String) = synchronized(lock) {
        val current = state ?: return@synchronized
        if (current.hash != hash) return@synchronized
        state = metadata(current, JSONObject(text))
        SdaPlaybackService.refresh()
    }
    fun begin(context: Context, title: String, hash: String = "", controls: Controls? = null): Long = synchronized(lock) {
        application = context.applicationContext
        val token = serial.incrementAndGet()
        val initial = State(token, title.take(200), true, hash = hash, controls = controls)
        state = queue[hash]?.let { metadata(initial, it) } ?: initial
        failure = ""
        PlaybackAudioFocus.begin(context, token)
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
        PlaybackAudioFocus.end(token)
        if (!setPlaying(token, false)) return
        Handler(Looper.getMainLooper()).postDelayed({ end(context, token) }, 5_000)
    }
    fun end(context: Context, token: Long): Boolean = synchronized(lock) {
        if (state?.token != token) return@synchronized false
        state = null
        PlaybackAudioFocus.end(token)
        context.applicationContext.stopService(Intent(context, SdaPlaybackService::class.java))
        true
    }
    fun diagnostics(): JSONObject = JSONObject()
        .put("active", state != null).put("serviceRunning", serviceRunning)
        .put("partialWakeLockHeld", wakeLockHeld).put("playing", state?.playing == true)
        .put("mediaSessionActive", mediaSessionActive).put("mediaStyleNotification", mediaSessionActive)
        .put("startFailureCategory", failure).put("audioFocus", PlaybackAudioFocus.diagnostics())
}

class SdaPlaybackService : Service() {
    private val handler = Handler(Looper.getMainLooper())
    private val commands = Executors.newSingleThreadExecutor { task -> Thread(task, "SDA media controls") }
    private var session: MediaSession? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private var wakeRenewedAt = 0L
    private var notificationState: PlaybackKeepAlive.State? = null
    private var notificationCanSkip = false
    private var progress = PlaybackKeepAlive.Progress(0, true)
    private var progressToken = 0L
    private var artworkUri = ""
    private var artworkToken = 0L
    private var artwork: Bitmap? = null
    private var destroyed = false
    private val poll = Runnable { readProgress() }
    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action == android.media.AudioManager.ACTION_AUDIO_BECOMING_NOISY) command("pause")
        }
    }
    companion object {
        private const val CHANNEL = "sda_playback"
        private const val NOTIFICATION = 48001
        private const val ACTION = "app.sda.mobile.MEDIA_CONTROL"
        @Volatile private var instance: SdaPlaybackService? = null
        internal fun focusChanged(token: Long, requestId: Long, playing: Boolean) {
            instance?.let { service -> service.execute {
                if (PlaybackKeepAlive.owns(token) && PlaybackAudioFocus.currentRequest(token, requestId))
                    PlaybackKeepAlive.snapshot()?.controls?.setInterrupted(playing)
            } }
        }
        internal fun refresh() {
            instance?.let { service -> service.handler.post { service.applyState() } }
        }
    }
    override fun onCreate() {
        super.onCreate()
        instance = this
        val noisyFilter = IntentFilter(android.media.AudioManager.ACTION_AUDIO_BECOMING_NOISY)
        if (Build.VERSION.SDK_INT >= 33) registerReceiver(noisyReceiver, noisyFilter, Context.RECEIVER_NOT_EXPORTED)
        else registerReceiver(noisyReceiver, noisyFilter)
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, "音乐播放", NotificationManager.IMPORTANCE_LOW).apply {
                description = "SDA 媒体播放控制"
                setSound(null, null); enableVibration(false)
            })
        wakeLock = getSystemService(PowerManager::class.java).newWakeLock(
            PowerManager.PARTIAL_WAKE_LOCK, "SDA:AudioPlayback").apply { setReferenceCounted(false) }
        session = MediaSession(this, "Spatial Decoder Atelier").apply {
            setFlags(MediaSession.FLAG_HANDLES_MEDIA_BUTTONS or MediaSession.FLAG_HANDLES_TRANSPORT_CONTROLS)
            setCallback(object : MediaSession.Callback() {
                override fun onPlay() = command("play")
                override fun onPause() = command("pause")
                override fun onStop() = command("stop")
                override fun onSkipToPrevious() = command("previous")
                override fun onSkipToNext() = command("next")
            }, handler)
            packageManager.getLaunchIntentForPackage(packageName)?.let { launch ->
                setSessionActivity(PendingIntent.getActivity(this@SdaPlaybackService, 0, launch,
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
            }
            setPlaybackToLocal(android.media.AudioAttributes.Builder()
                .setUsage(android.media.AudioAttributes.USAGE_MEDIA)
                .setContentType(android.media.AudioAttributes.CONTENT_TYPE_MUSIC).build())
            isActive = true
        }
        PlaybackKeepAlive.mediaSessionActive = true
        PlaybackKeepAlive.serviceRunning = true
        handler.post(poll)
    }
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        applyState()
        if (intent?.action == ACTION && PlaybackKeepAlive.owns(intent.getLongExtra("token", 0))) {
            command(intent.getStringExtra("command").orEmpty())
        }
        return START_NOT_STICKY
    }
    private fun execute(task: () -> Unit) {
        try { commands.execute { try { task() } catch (error: Exception) {
            Log.w("SdaPlayback", "Media command failed: " + error.javaClass.simpleName)
        } } } catch (_: RejectedExecutionException) { }
    }
    private fun command(name: String) {
        val state = PlaybackKeepAlive.snapshot() ?: return
        execute {
            if (!PlaybackKeepAlive.owns(state.token)) return@execute
            val control = state.controls ?: return@execute
            when (name) {
                "play" -> control.setPlaying(true)
                "pause" -> control.setPlaying(false)
                "stop" -> control.stop()
                "previous" -> if (PlaybackKeepAlive.canSkip()) control.skip(-1)
                "next" -> if (PlaybackKeepAlive.canSkip()) control.skip(1)
            }
        }
    }
    private fun action(state: PlaybackKeepAlive.State, name: String, title: String, icon: Int): Notification.Action {
        val intent = Intent(this, SdaPlaybackService::class.java).setAction(ACTION)
            .putExtra("token", state.token).putExtra("command", name)
        // Old notifications must never control a replacement track.
        intent.data = Uri.parse("sda://media/${state.token}/$name")
        val pending = PendingIntent.getService(this, name.hashCode(), intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        return Notification.Action.Builder(icon, title, pending).build()
    }
    private fun notification(state: PlaybackKeepAlive.State): Notification {
        val builder = Notification.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle(state.title.ifBlank { "Spatial Decoder Atelier" })
            .setContentText(state.artist.ifBlank { if (state.playing) "空间音频播放中" else "播放已暂停" })
            .setSubText(state.album.takeIf { it.isNotBlank() })
            .setCategory(Notification.CATEGORY_TRANSPORT).setVisibility(Notification.VISIBILITY_PUBLIC)
            .setOnlyAlertOnce(true).setOngoing(state.playing).setShowWhen(false)
        artwork?.let { builder.setLargeIcon(it) }
        val skip = PlaybackKeepAlive.canSkip()
        if (skip) builder.addAction(action(state, "previous", "上一首", android.R.drawable.ic_media_previous))
        builder.addAction(action(state, if (state.playing) "pause" else "play",
            if (state.playing) "暂停" else "播放", if (state.playing) android.R.drawable.ic_media_pause else android.R.drawable.ic_media_play))
        if (skip) builder.addAction(action(state, "next", "下一首", android.R.drawable.ic_media_next))
        val style = Notification.MediaStyle().setMediaSession(session!!.sessionToken)
        if (skip) style.setShowActionsInCompactView(0, 1, 2) else style.setShowActionsInCompactView(0)
        builder.setStyle(style)
        session?.controller?.sessionActivity?.let { builder.setContentIntent(it) }
        if (Build.VERSION.SDK_INT >= 31) builder.setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE)
        return builder.build()
    }
    private fun publishTransport(state: PlaybackKeepAlive.State) {
        val p = if (progressToken == state.token) progress else PlaybackKeepAlive.Progress(0, true)
        val actions = PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or PlaybackState.ACTION_PLAY_PAUSE or
            PlaybackState.ACTION_STOP or (if (PlaybackKeepAlive.canSkip()) PlaybackState.ACTION_SKIP_TO_NEXT or PlaybackState.ACTION_SKIP_TO_PREVIOUS else 0L)
        session?.setPlaybackState(PlaybackState.Builder().setActions(actions).setState(
            if (!state.playing) PlaybackState.STATE_PAUSED else if (p.preparing) PlaybackState.STATE_BUFFERING else PlaybackState.STATE_PLAYING,
            p.positionMs, if (state.playing && !p.preparing) 1f else 0f, SystemClock.elapsedRealtime()).build())
    }
    private fun applyState() {
        if (destroyed) return
        val state = PlaybackKeepAlive.snapshot()
        if (state == null) { stopSelf(); return }
        val changed = notificationState != state || notificationCanSkip != PlaybackKeepAlive.canSkip()
        if (changed) {
            val metadata = AndroidMediaMetadata.Builder()
                .putString(AndroidMediaMetadata.METADATA_KEY_MEDIA_ID, state.hash)
                .putString(AndroidMediaMetadata.METADATA_KEY_TITLE, state.title)
                .putString(AndroidMediaMetadata.METADATA_KEY_ARTIST, state.artist)
                .putString(AndroidMediaMetadata.METADATA_KEY_ALBUM, state.album)
                .putLong(AndroidMediaMetadata.METADATA_KEY_DURATION, state.durationMs)
            if (artworkToken != state.token || artworkUri != state.coverUri) {
                artworkToken = state.token; artwork = null; artworkUri = state.coverUri; loadArtwork(state)
            }
            artwork?.let { metadata.putBitmap(AndroidMediaMetadata.METADATA_KEY_ALBUM_ART, it) }
            session?.setMetadata(metadata.build())
        }
        publishTransport(state)
        if (changed) {
            val notification = notification(state)
            if (Build.VERSION.SDK_INT >= 29) startForeground(NOTIFICATION, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
            else startForeground(NOTIFICATION, notification)
            notificationState = state
            notificationCanSkip = PlaybackKeepAlive.canSkip()
        }
        val lock = wakeLock
        if (state.playing && (lock?.isHeld != true || SystemClock.elapsedRealtime() - wakeRenewedAt >= 5 * 60 * 1000L)) {
            lock?.acquire(10 * 60 * 1000L); wakeRenewedAt = SystemClock.elapsedRealtime()
        } else if (!state.playing && lock?.isHeld == true) lock.release()
        PlaybackKeepAlive.wakeLockHeld = lock?.isHeld == true
    }
    private fun readProgress() {
        val state = PlaybackKeepAlive.snapshot() ?: return
        execute {
            val current = runCatching { state.controls?.progress() }.getOrNull()
            handler.post {
                if (destroyed) return@post
                if (PlaybackKeepAlive.owns(state.token) && current != null) { progress = current; progressToken = state.token }
                applyState()
                handler.removeCallbacks(poll); handler.postDelayed(poll, 1_000)
            }
        }
    }
    private fun loadArtwork(state: PlaybackKeepAlive.State) {
        if (state.coverUri.isEmpty()) return
        execute {
            val uri = Uri.parse(state.coverUri)
            // Local imported artwork only; bounded decode off main/audio threads.
            val bitmap = if (uri.scheme in listOf("file", "content")) try {
                val bytes = contentResolver.openInputStream(uri)?.use { input ->
                    val output = java.io.ByteArrayOutputStream()
                    val buffer = ByteArray(16 * 1024)
                    while (output.size() <= 16 * 1024 * 1024) {
                        val n = input.read(buffer)
                        if (n < 0) break
                        output.write(buffer, 0, n)
                    }
                    output.toByteArray()
                }
                if (bytes == null || bytes.size > 16 * 1024 * 1024) null else {
                    val options = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)
                    if (options.outWidth <= 0 || options.outHeight <= 0) null else {
                        options.inJustDecodeBounds = false; options.inSampleSize = 1
                        while (maxOf(options.outWidth, options.outHeight) / options.inSampleSize > 512) options.inSampleSize *= 2
                        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)
                    }
                }
            } catch (_: Exception) { null } else null
            handler.post {
                val current = PlaybackKeepAlive.snapshot()
                if (destroyed || current?.token != state.token || current.coverUri != state.coverUri) { bitmap?.recycle(); return@post }
                artwork = bitmap; notificationState = null; applyState()
            }
        }
    }
    override fun onDestroy() {
        destroyed = true
        unregisterReceiver(noisyReceiver)
        handler.removeCallbacksAndMessages(null)
        commands.shutdownNow()
        session?.isActive = false; session?.release(); session = null
        wakeLock?.let { if (it.isHeld) it.release() }
        PlaybackKeepAlive.wakeLockHeld = false
        PlaybackKeepAlive.mediaSessionActive = false
        PlaybackKeepAlive.serviceRunning = false
        if (instance === this) instance = null
        stopForeground(STOP_FOREGROUND_REMOVE)
        super.onDestroy()
    }
    override fun onBind(intent: Intent?): IBinder? = null
}
