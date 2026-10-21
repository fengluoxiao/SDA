package app.sda.mobile

import android.content.Intent
import android.os.SystemClock
import android.util.Log
import android.view.accessibility.AccessibilityNodeInfo
import androidx.test.platform.app.InstrumentationRegistry
import app.sda.mobile.sda.PlaybackKeepAlive
import com.sda.nativebridge.SdaEngine
import java.io.File
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class BackgroundAudioTest {
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private fun await(message: String, condition: () -> Boolean) {
        val end = SystemClock.elapsedRealtime() + 5_000
        while (!condition() && SystemClock.elapsedRealtime() < end) SystemClock.sleep(20)
        assertTrue(message, condition())
    }
    private fun showPlayer() {
        context.packageManager.getLaunchIntentForPackage(context.packageName)?.let {
            context.startActivity(it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        }
        SystemClock.sleep(1_000)
    }
    @Test fun foregroundLeaseReleasesOnPauseAndRejectsStaleTrackEnd() {
        showPlayer()
        var current = PlaybackKeepAlive.begin(context, "SDA lifecycle verification")
        try {
            await("Media service and partial CPU lease must become active") {
                PlaybackKeepAlive.serviceRunning && PlaybackKeepAlive.wakeLockHeld
            }
            assertTrue(PlaybackKeepAlive.setPlaying(current, false))
            await("Pause must release the CPU lease") { !PlaybackKeepAlive.wakeLockHeld }
            assertTrue(PlaybackKeepAlive.setPlaying(current, true))
            await("Resume must renew the CPU lease") { PlaybackKeepAlive.wakeLockHeld }
            val old = current
            current = PlaybackKeepAlive.begin(context, "SDA next-track verification")
            assertFalse("Late EOF must not stop a new song", PlaybackKeepAlive.end(context, old))
            await("New track still owns the service") { PlaybackKeepAlive.serviceRunning && PlaybackKeepAlive.wakeLockHeld }
        } finally {
            PlaybackKeepAlive.end(context, current)
        }
        await("Stop must release foreground service and wake lock") {
            !PlaybackKeepAlive.serviceRunning && !PlaybackKeepAlive.wakeLockHeld
        }
    }
    private fun extractAssets(root: String, destination: File) {
        destination.mkdirs()
        for (name in context.assets.list(root).orEmpty()) {
            val children = context.assets.list("$root/$name").orEmpty()
            val target = File(destination, name)
            if (children.isNotEmpty()) extractAssets("$root/$name", target)
            else context.assets.open("$root/$name").use { input -> target.outputStream().use { input.copyTo(it) } }
        }
    }
    private fun searchControl(node: AccessibilityNodeInfo?): AccessibilityNodeInfo? {
        if (node == null) return null
        val label = "${node.text?.toString().orEmpty()} ${node.contentDescription?.toString().orEmpty()}"
        if (node.isClickable && Regex("search|搜索|搜尋", RegexOption.IGNORE_CASE).containsMatchIn(label)) return node
        for (i in 0 until node.childCount) searchControl(node.getChild(i))?.let { return it }
        return null
    }
    private fun scroll(node: AccessibilityNodeInfo?): Boolean {
        if (node == null) return false
        if (node.isScrollable && node.performAction(AccessibilityNodeInfo.ACTION_SCROLL_FORWARD)) return true
        for (i in 0 until node.childCount) if (scroll(node.getChild(i))) return true
        return false
    }
    @Test fun backgroundPlaybackSurvivesSettingsInteraction() {
        val args = InstrumentationRegistry.getArguments()
        val legacy = args.getString("allowLegacy") == "true"
        val variant = (args.getString("variant") ?: "candidate").also { require(it.matches(Regex("[A-Za-z0-9_-]{1,32}"))) }
        val input = File(context.getExternalFilesDir(null), "background-gunjo.mhas")
        assertTrue("Explicit user audio must be supplied externally; it must not be packaged", input.isFile)
        showPlayer()
        val lease = if (legacy) 0L else PlaybackKeepAlive.begin(context, "SDA background stress verification")
        val dir = File(context.cacheDir, "background-verification-hrtf")
        var handle = 0L
        val rows = JSONArray()
        var maximumMissingDelta = 0L
        try {
            extractAssets("hrtf-restored/hrtf-dense", dir)
            handle = SdaEngine.nativeInit(JSONObject().put("sampleRate", 48000).put("outputChannels", 2)
                .put("layout", "360RA-13").put("directObjectHrtf", true).put("directionalHrtf", true)
                .put("binauralEnabled", true).put("objectRendering", true).put("hrtfWetWeight", 0).toString(), File(dir,"hrtf-set.json").path)
            assertTrue(SdaEngine.nativeInitError(), handle != 0L)
            assertEquals(0, SdaEngine.nativeStart(handle))
            assertEquals(0, SdaEngine.nativeOpenMpegh(handle))
            assertEquals("", SdaEngine.nativeSetVolumeBalance(handle, true))
            assertEquals("", SdaEngine.nativeSetSpatialEnhancement(handle, true))
            // A verified complete pre-limiter measurement for this supplied
            // fixture, exercising stable cached normalization from sample zero.
            if (args.getString("useCache", "true") == "true") assertEquals("", SdaEngine.nativeSetMeasuredLoudness(handle,
                "{\"integratedLufs\":-1.232887149,\"blocks\":2456,\"truePeakDbtp\":11.84143249}"))
            val started = SystemClock.elapsedRealtime()
            var lastRow = started
            var baselineMissing: Long? = null
            var settingsOpened = false
            var searched = false
            var firstProgrammeGain: Double? = null
            var firstConsumedMs: Long? = null
            input.inputStream().use { source ->
                val buffer = ByteArray(24*1024)
                while (SystemClock.elapsedRealtime() - started < 20_000) {
                    val state = JSONObject(SdaEngine.nativeStatus(handle))
                    val decoded = state.optLong("decodedSamplePos")
                    val consumed = state.optLong("consumedSamplePos")
                    if (decoded-consumed < 4*48_000) {
                        val n=source.read(buffer)
                        if(n>0) assertTrue(SdaEngine.nativeLastError(), SdaEngine.nativeFeed(handle,buffer.copyOf(n))>=0)
                    }
                    val now=SystemClock.elapsedRealtime()
                    if(now-started>3_000 && !settingsOpened) {
                        context.startActivity(Intent(android.provider.Settings.ACTION_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                        settingsOpened=true
                        baselineMissing=state.optLong("callbackMissingFrames",-1)
                    }
                    if(now-lastRow>=500) {
                        val device=state.optJSONObject("outputDevice")
                        assertTrue("Audio device failed: $device",device?.optInt("errorCode",0)?.let{it>=0}!=false)
                        if(!legacy) assertEquals("aaudio-callback",device?.optString("backend"))
                        if(settingsOpened) {
                            val root=instrumentation.uiAutomation.rootInActiveWindow
                            if(root?.packageName?.toString()=="com.android.settings") {
                                if(!searched) searched=searchControl(root)?.performAction(AccessibilityNodeInfo.ACTION_CLICK)==true
                                else scroll(root)
                            }
                            val missing=state.optLong("callbackMissingFrames",-1)
                            if(missing>=0 && baselineMissing!=null && baselineMissing!!>=0) maximumMissingDelta=maxOf(maximumMissingDelta,missing-baselineMissing!!)
                        }
                        if (consumed>0 && !legacy) {
                            if(firstConsumedMs==null) firstConsumedMs=now-started
                            val gain=state.getDouble("renderedProgramGainDb")
                            if(firstProgrammeGain==null) firstProgrammeGain=gain
                            assertEquals("Programme normalization must not ride the music",firstProgrammeGain!!,gain,0.02)
                        }
                        rows.put(JSONObject().put("elapsedMs",now-started).put("state",state)
                            .put("backgroundLease",if(legacy)JSONObject()else PlaybackKeepAlive.diagnostics()))
                        lastRow=now
                    }
                    SystemClock.sleep(5)
                }
            }
            val status=JSONObject(SdaEngine.nativeStatus(handle))
            assertTrue("Device clock stopped",status.getLong("consumedSamplePos")>10*48_000)
            if(!legacy) assertEquals("No source FIFO starvation under UI interaction",0L,maximumMissingDelta)
            val report=JSONObject().put("variant",variant).put("legacy",legacy)
                .put("settingsOpened",settingsOpened).put("searchControlClicked",searched)
                .put("firstConsumedMs",firstConsumedMs).put("fixedProgrammeGainDb",firstProgrammeGain)
                .put("deviceModel",android.os.Build.MODEL).put("deviceManufacturer",android.os.Build.MANUFACTURER)
                .put("maximumMissingFramesDelta",maximumMissingDelta).put("states",rows)
            File(context.getExternalFilesDir(null),"background-stress-$variant.json").writeText(report.toString(2))
            Log.i("SdaBackgroundTest",report.toString())
        } finally {
            if(handle!=0L) SdaEngine.nativeClose(handle)
            if(!legacy) PlaybackKeepAlive.end(context,lease)
            dir.deleteRecursively()
        }
    }
}
