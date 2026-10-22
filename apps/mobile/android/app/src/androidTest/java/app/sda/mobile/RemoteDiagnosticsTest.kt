package app.sda.mobile

import android.os.SystemClock
import androidx.test.platform.app.InstrumentationRegistry
import app.sda.mobile.sda.RemoteDiagnostics
import com.sda.nativebridge.SdaEngine
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.io.File
import java.net.Socket

class RemoteDiagnosticsTest {
    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext
    private fun await(message: String, condition: () -> Boolean) {
        val end = SystemClock.elapsedRealtime() + 8_000
        while (!condition() && SystemClock.elapsedRealtime() < end) SystemClock.sleep(20)
        assertTrue(message, condition())
    }
    private fun request(port: Int, text: String = "GET /diagnostics HTTP/1.1\r\nHost: localhost\r\n\r\n"): String = Socket("127.0.0.1", port).use {
        it.soTimeout = 6_000
        it.getOutputStream().write(text.toByteArray())
        it.getInputStream().readBytes().toString(Charsets.UTF_8)
    }
    private fun json(server: RemoteDiagnostics) = JSONObject(request(server.listeningPort).substringAfter("\r\n\r\n"))
    private fun metadata() = JSONObject().put("generation", 1).put("playing", true).put("paused", false).put("background", true)
        .put("route", "SDA-HRTFHQ").put("input", JSONObject().put("codec", "mpegh").put("layout", "360RA-13"))

    @Test fun compatibleReadOnlyProtocolRedactsAndBoundsHistory() {
        val server = RemoteDiagnostics(context, 0, 20)
        assertFalse(server.isEnabled)
        assertEquals(0, server.listeningPort)
        try {
            server.configure(true) { metadata() }
            await("Server binds and publishes schema") { server.listeningPort > 0 && runCatching { json(server).optInt("schema") == 1 }.getOrDefault(false) }
            val privateText = "NEVER_EXPOSE_track_path_or_title"
            val native = JSONObject().put("callbackMissingFrames", 10).put("positionMs", 1000).put("fifoFrames", 200)
                .put("renderTotalMicros", 2345).put("renderBlocks", 9).put("title", privateText).put("path", privateText)
                .put("outputDevice", JSONObject().put("hardwareXruns", 2).put("backend", "aaudio-callback").put("deviceName", privateText))
                .put("rendererConfiguration", JSONObject().put("sampleRate", 48000).put("hrtfPath", privateText))
            server.observeNative(native, 1)
            await("Native telemetry reaches cached HTTP snapshot") { json(server).getJSONObject("playback").optLong("renderTotalMicros") == 2345L }
            for (i in 0..110) server.event("pause", 1)
            await("Events and history retain bounded records") { json(server).getJSONArray("events").length() == 96 && json(server).getJSONArray("history").length() == 60 }
            val response = request(server.listeningPort)
            assertFalse(response.contains(privateText))
            assertTrue(response.contains("Cache-Control: no-store"))
            assertTrue(response.startsWith("HTTP/1.1 200"))
            assertTrue(request(server.listeningPort, "GET / HTTP/1.1\r\n\r\n").startsWith("HTTP/1.1 200"))
            assertTrue(request(server.listeningPort, "POST /diagnostics HTTP/1.1\r\n\r\n").startsWith("HTTP/1.1 405"))
            assertTrue(request(server.listeningPort, "GET /pause HTTP/1.1\r\n\r\n").startsWith("HTTP/1.1 404"))
            assertTrue(request(server.listeningPort, "malformed\r\n\r\n").startsWith("HTTP/1.1 400"))
            assertTrue(runCatching { request(server.listeningPort, "GET /diagnostics HTTP/1.1\r\nX: " + "a".repeat(8300)) }.getOrDefault("").isEmpty())
            // Partial TCP receives and split request terminators work like iOS.
            Socket("127.0.0.1", server.listeningPort).use {
                it.soTimeout = 2_000
                it.getOutputStream().write("GET /diagnostics HTTP/1.1\r\nHost: localhost\r\n".toByteArray())
                SystemClock.sleep(50)
                it.getOutputStream().write("\r\n".toByteArray())
                assertTrue(it.getInputStream().readBytes().toString(Charsets.UTF_8).startsWith("HTTP/1.1 200"))
            }
            File(context.getExternalFilesDir(null), "android-diagnostics-test.json").writeText(json(server).toString())
        } finally { server.configure(false) { JSONObject() } }
        assertFalse(server.isEnabled)
        assertEquals("已关闭", server.status)
    }
    @Test fun disableClosesIdleClientsAndRapidRestartKeepsNewEpoch() {
        val server = RemoteDiagnostics(context, 0)
        val sockets = mutableListOf<Socket>()
        try {
            server.configure(true) { metadata() }
            await("Listen") { server.listeningPort > 0 }
            repeat(4) { sockets.add(Socket("127.0.0.1", server.listeningPort).apply {
                soTimeout = 2_000; getOutputStream().write("GET /".toByteArray())
            }) }
            SystemClock.sleep(100)
            Socket("127.0.0.1", server.listeningPort).use { it.soTimeout = 1_000; assertEquals(-1, it.getInputStream().read()) }
            server.configure(false) { JSONObject() }
            for (socket in sockets) assertTrue(runCatching { socket.getInputStream().read() == -1 }.getOrDefault(true))
            repeat(8) { server.configure(true) { metadata() }; server.configure(false) { JSONObject() } }
            server.configure(true) { metadata() }
            await("New epoch survives previous asynchronous listener shutdown") {
                server.isEnabled && server.listeningPort > 0 && runCatching { json(server).optInt("schema") == 1 }.getOrDefault(false)
            }
        } finally { sockets.forEach { it.close() }; server.configure(false) { JSONObject() } }
    }
    @Test fun packagedNativeBridgeReportsRenderStagesWithoutStartingAudio() {
        val dir = File(context.cacheDir, "diagnostics-test-hrtf")
        fun extract(root: String, output: File) {
            output.mkdirs()
            for (name in context.assets.list(root).orEmpty()) {
                val asset = "$root/$name"; val dst = File(output, name)
                if (context.assets.list(asset).orEmpty().isNotEmpty()) extract(asset, dst)
                else context.assets.open(asset).use { input -> dst.outputStream().use { input.copyTo(it) } }
            }
        }
        extract("hrtf-restored/hrtf-dense", dir)
        val handle = SdaEngine.nativeInit("{\"sampleRate\":48000,\"outputChannels\":2}", File(dir, "hrtf-set.json").path)
        assertTrue(SdaEngine.nativeInitError(), handle != 0L)
        try {
            val status = JSONObject(SdaEngine.nativeStatus(handle))
            assertTrue(status.has("renderTotalMicros"))
            assertEquals(7, status.getJSONArray("renderWorstStagesMicros").length())
            assertTrue(status.getString("rendererArchitecture") in listOf("aarch64", "x86_64"))
            assertEquals(48000, status.getJSONObject("rendererConfiguration").getInt("sampleRate"))
        } finally { SdaEngine.nativeClose(handle) }
    }
}
