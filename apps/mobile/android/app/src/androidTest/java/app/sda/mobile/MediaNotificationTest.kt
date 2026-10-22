package app.sda.mobile

import android.app.Notification
import android.app.NotificationManager
import android.content.Intent
import android.graphics.Bitmap
import android.media.MediaMetadata
import android.media.session.MediaController
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.SystemClock
import androidx.test.platform.app.InstrumentationRegistry
import app.sda.mobile.sda.PlaybackKeepAlive
import java.io.File
import java.util.concurrent.atomic.AtomicInteger
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class MediaNotificationTest {
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private fun await(message: String, condition: () -> Boolean) {
        val end = SystemClock.elapsedRealtime() + 8_000
        while (!condition() && SystemClock.elapsedRealtime() < end) SystemClock.sleep(30)
        assertTrue(message, condition())
    }
    @Suppress("DEPRECATION")
    @Test fun mediaStyleTokenMetadataAndTransportControlsAreRegistered() {
        context.packageManager.getLaunchIntentForPackage(context.packageName)?.let {
            context.startActivity(it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        }
        SystemClock.sleep(1_000)
        val cover = File(context.cacheDir, "media-session-test-cover.png")
        val bitmap = Bitmap.createBitmap(96, 96, Bitmap.Config.ARGB_8888)
        bitmap.eraseColor(android.graphics.Color.CYAN)
        cover.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }; bitmap.recycle()
        val hash = "a".repeat(64)
        val metadata = JSONObject().put("title", "媒体注册验证").put("artist", "SDA artist")
            .put("album", "SDA album").put("durationMs", 120_000).put("coverUri", cover.toURI().toString())
        PlaybackKeepAlive.configureQueue(JSONArray()
            .put(JSONObject().put("hash", hash).put("metadata", metadata))
            .put(JSONObject().put("hash", "b".repeat(64))).toString())
        var token = 0L
        val skips = AtomicInteger()
        val controls = object : PlaybackKeepAlive.Controls {
            override fun setPlaying(playing: Boolean) = PlaybackKeepAlive.setPlaying(token, playing)
            override fun stop() { PlaybackKeepAlive.end(context, token) }
            override fun skip(direction: Int) { skips.addAndGet(direction) }
            override fun progress() = PlaybackKeepAlive.Progress(12_345)
        }
        token = PlaybackKeepAlive.begin(context, "fallback.mhas", hash, controls)
        val manager = context.getSystemService(NotificationManager::class.java)
        fun current() = manager.activeNotifications.firstOrNull { it.id == 48001 }?.notification
        try {
            await("MediaStyle notification must contain a system MediaSession token") {
                current()?.extras?.getParcelable<MediaSession.Token>(Notification.EXTRA_MEDIA_SESSION) != null
            }
            val notification = current()!!
            assertEquals("android.app.Notification\$MediaStyle", notification.extras.getString(Notification.EXTRA_TEMPLATE))
            assertEquals(3, notification.actions.size)
            val sessionToken = notification.extras.getParcelable<MediaSession.Token>(Notification.EXTRA_MEDIA_SESSION)!!
            val controller = MediaController(context, sessionToken)
            await("Active media session must publish metadata, artwork and playback state") {
                PlaybackKeepAlive.mediaSessionActive && controller.metadata?.getString(MediaMetadata.METADATA_KEY_TITLE) == "媒体注册验证" &&
                controller.metadata?.getBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART) != null &&
                controller.playbackState?.state == PlaybackState.STATE_PLAYING
            }
            assertEquals("SDA artist", controller.metadata.getString(MediaMetadata.METADATA_KEY_ARTIST))
            assertEquals(120_000L, controller.metadata.getLong(MediaMetadata.METADATA_KEY_DURATION))
            assertEquals(12_345L, controller.playbackState.position)
            instrumentation.uiAutomation.executeShellCommand("cmd statusbar expand-settings").close()
            SystemClock.sleep(1_000)
            instrumentation.uiAutomation.takeScreenshot()?.let { image ->
                File(context.getExternalFilesDir(null), "media-session-shade.png").outputStream().use {
                    image.compress(Bitmap.CompressFormat.PNG, 100, it)
                }; image.recycle()
            }
            instrumentation.uiAutomation.executeShellCommand("cmd statusbar collapse").close()
            controller.transportControls.pause()
            await("System pause must update playback state and release wake lock") {
                controller.playbackState?.state == PlaybackState.STATE_PAUSED && !PlaybackKeepAlive.wakeLockHeld
            }
            controller.transportControls.play()
            await("System play must resume state and renew wake lock") {
                controller.playbackState?.state == PlaybackState.STATE_PLAYING && PlaybackKeepAlive.wakeLockHeld
            }
            controller.transportControls.skipToNext()
            await("System next must reach the player's queue command") { skips.get() == 1 }
            controller.transportControls.skipToPrevious()
            await("System previous must reach the player's queue command") { skips.get() == 0 }
            // Notification buttons use the same controls as the system media card.
            current()!!.actions[1].actionIntent.send()
            await("Notification pause PendingIntent must reach media controls") { controller.playbackState?.state == PlaybackState.STATE_PAUSED }
            await("Paused notification exposes play") { current()?.actions?.get(1)?.title?.toString() == "播放" }
            current()!!.actions[1].actionIntent.send()
            await("Notification play PendingIntent must reach media controls") { controller.playbackState?.state == PlaybackState.STATE_PLAYING }
            await("Resumed notification exposes pause") { current()?.actions?.get(1)?.title?.toString() == "暂停" }
            val oldPause = current()!!.actions[1].actionIntent
            val oldToken = token
            token = PlaybackKeepAlive.begin(context, "replacement", hash, controls)
            await("Replacement track has an active session") { PlaybackKeepAlive.mediaSessionActive && PlaybackKeepAlive.wakeLockHeld }
            oldPause.send()
            SystemClock.sleep(250)
            assertTrue("Stale notification must not pause the replacement", PlaybackKeepAlive.diagnostics().getBoolean("playing"))
            assertFalse(PlaybackKeepAlive.end(context, oldToken))
            controller.transportControls.stop()
            await("System stop must release the session, service and CPU lease") {
                !PlaybackKeepAlive.serviceRunning && !PlaybackKeepAlive.mediaSessionActive && !PlaybackKeepAlive.wakeLockHeld
            }
        } finally {
            PlaybackKeepAlive.end(context, token)
            PlaybackKeepAlive.configureQueue("[]")
            cover.delete()
        }
    }
}
