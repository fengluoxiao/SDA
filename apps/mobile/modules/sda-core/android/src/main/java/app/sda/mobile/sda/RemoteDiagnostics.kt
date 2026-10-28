package app.sda.mobile.sda

import android.content.Context
import android.os.Build
import android.os.PowerManager
import android.os.Process
import android.os.SystemClock
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.net.InetSocketAddress
import java.net.NetworkInterface
import java.net.ServerSocket
import java.net.Socket
import java.util.Collections
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledExecutorService
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit

/** Same opt-in, read-only HTTP interface as iOS. No uploads, control API,
 * audio, track identity, file paths or persistent diagnostic cache. */
class RemoteDiagnostics(private val context: Context, private val port: Int = 8765, private val sampleIntervalMillis: Long = 1000) {
    private data class NativeSample(val json: String, val elapsed: Long, val generation: Long)
    private val lock = Any()
    private val clients = Collections.synchronizedSet(mutableSetOf<Socket>())
    private val events = ArrayDeque<JSONObject>()
    private val history = ArrayDeque<JSONObject>()
    @Volatile private var nativeSample = NativeSample("{}", 0, -1)
    private data class Published(val json: String, val elapsed: Long)
    @Volatile private var published = Published("{}", 0)
    @Volatile private var epoch = 0L
    @Volatile private var enabled = false
    @Volatile private var detail = "已关闭"
    @Volatile private var boundPort = 0
    @Volatile private var server: ServerSocket? = null
    private var sampler: ScheduledExecutorService? = null
    private var workers: ThreadPoolExecutor? = null
    private var lastSampleAt = 0L
    private var lastGeneration = -1L
    private var previousMissing = 0L
    private var previousXruns = 0L
    private var cachedThreads = JSONArray()
    private var threadsAt = 0L
    val isEnabled: Boolean get() = enabled
    val status: String get() = detail
    val listeningPort: Int get() = boundPort

    companion object {
        private val playbackKeys = listOf("positionMs", "fifoFrames", "pendingBatches", "decodedSamplePos", "consumedSamplePos",
            "sourceCodec", "outputChannels", "outputLayout", "paused", "preparingAudio", "underruns", "callbackMissingFrames",
            "outputCallbacks", "renderBlocks", "renderTotalMicros", "renderMaxMicros", "renderWorstStagesMicros", "rendererRevision", "rendererArchitecture",
            "spatialBalanceAlgorithm", "stereoBassPreservationActive", "stereoBassPreservationAlgorithm", "liveLoudnessDucking", "fftBackend", "programmeBalancePolicy", "volumeBalanceEnabled", "volumeBalanceEligible",
            "programGainDb", "renderedProgramGainDb", "rendererProgramEnabled", "spatialEnhancementEnabled", "auxiliaryBalanceEnabled",
            "auxiliaryBalanceLevels", "temporalDetailEnabled", "temporalDetailLevels", "listenerYawDegrees", "binauralEnabled",
            "objectRendering", "objectConvolverCount", "continuousObjectCount", "hrtfReady", "hrtfDirections")
        private val deviceKeys = listOf("backend", "hardwareXruns", "bufferFrames", "burstFrames", "performanceMode", "errorCode", "recovering", "restarts")
    }
    /** Called on already-existing status reads; never asks the renderer for data.
     * Throttle before copying, so enabling diagnostics adds no native polling. */
    fun observeNative(value: JSONObject, generation: Long) {
        if (!enabled) return
        val now = SystemClock.elapsedRealtime()
        if (nativeSample.generation == generation && now - nativeSample.elapsed < 250) return
        val safe = JSONObject()
        for (key in playbackKeys) if (value.has(key)) safe.put(key, value.get(key))
        value.optJSONObject("outputDevice")?.let { source ->
            val output = JSONObject()
            for (key in deviceKeys) if (source.has(key)) output.put(key, source.get(key))
            safe.put("outputDevice", output)
        }
        value.optJSONObject("rendererConfiguration")?.let { source ->
            val configuration = JSONObject()
            for (key in listOf("sampleRate", "outputChannels", "layout", "directObjectHrtf", "directionalHrtf", "binauralEnabled", "objectRendering", "hrtfWetWeight"))
                if (source.has(key)) configuration.put(key, source.get(key))
            safe.put("rendererConfiguration", configuration)
        }
        nativeSample = NativeSample(safe.toString(), now, generation)
    }
    fun event(name: String, generation: Long) = synchronized(lock) {
        if (!enabled) return@synchronized
        events.addLast(JSONObject().put("time", System.currentTimeMillis() / 1000.0).put("event", name).put("generation", generation))
        while (events.size > 96) events.removeFirst()
    }
    @Synchronized fun configure(next: Boolean, metadata: () -> JSONObject) {
        epoch += 1
        val serial = epoch
        enabled = false
        server?.runCatching { close() }; server = null
        synchronized(clients) { clients.toList().forEach { it.runCatching { close() } }; clients.clear() }
        workers?.shutdownNow(); workers = null
        sampler?.shutdownNow(); sampler = null
        synchronized(lock) { events.clear(); history.clear() }
        nativeSample = NativeSample("{}", 0, -1); published = Published("{}", 0); lastSampleAt = 0
        lastGeneration = -1; threadsAt = 0; cachedThreads = JSONArray(); boundPort = 0
        detail = "已关闭"
        if (!next) return
        detail = "正在启动"
        enabled = true
        val pool = ThreadPoolExecutor(4, 4, 15, TimeUnit.SECONDS, ArrayBlockingQueue<Runnable>(1),
            { task -> Thread(task, "SDA diagnostics HTTP") }, ThreadPoolExecutor.AbortPolicy())
        workers = pool
        sampler = Executors.newSingleThreadScheduledExecutor { task -> Thread(task, "SDA diagnostics sample").apply { priority = Thread.MIN_PRIORITY } }
        sampler!!.scheduleWithFixedDelay({
            if (epoch == serial && enabled) try { sample(metadata, serial) } catch (_: Exception) { }
        }, 0, sampleIntervalMillis, TimeUnit.MILLISECONDS)
        Thread({
            var listener: ServerSocket? = null
            try {
                val active = ServerSocket().apply { reuseAddress = true; bind(InetSocketAddress(port), 4) }
                listener = active
                synchronized(this) {
                    if (epoch != serial || !enabled) { active.close(); return@Thread }
                    server = active; boundPort = active.localPort
                    detail = "服务已开启 · 端口 $boundPort · /diagnostics"
                }
                while (epoch == serial && enabled) {
                    val socket = active.accept()
                    if (clients.size >= 4) { socket.close(); continue }
                    clients.add(socket)
                    try { pool.execute { respond(socket, serial) } }
                    catch (_: Exception) { clients.remove(socket); socket.close() }
                }
            } catch (_: Exception) {
                synchronized(this) {
                    if (epoch == serial && enabled) {
                        detail = "监听失败 · 请关闭后重新开启"; enabled = false; boundPort = 0; server = null
                        synchronized(clients) { clients.toList().forEach { it.runCatching { close() } }; clients.clear() }
                        sampler?.shutdownNow(); pool.shutdownNow()
                    }
                }
            } finally { listener?.runCatching { close() } }
        }, "SDA diagnostics listen").apply { priority = Thread.MIN_PRIORITY; start() }
    }
    private fun sample(metadata: () -> JSONObject, serial: Long) {
        val now = SystemClock.elapsedRealtime()
        val meta = metadata()
        val sample = nativeSample
        val generation = meta.optLong("generation")
        val playback = if (sample.generation == generation) JSONObject(sample.json) else JSONObject()
        meta.optJSONObject("input")?.let { input ->
            playback.put("sourceCodec", input.optString("codec"))
            playback.put("outputLayout", input.optString("layout"))
            playback.put("outputChannels", 2)
        }
        val power = context.getSystemService(PowerManager::class.java)
        val memory = Runtime.getRuntime()
        if (now - threadsAt >= 5_000 || threadsAt == 0L) { cachedThreads = threads(); threadsAt = now }
        val android = JSONObject().put("manufacturer", Build.MANUFACTURER).put("model", Build.MODEL)
            .put("apiLevel", Build.VERSION.SDK_INT).put("hardware", Build.HARDWARE)
            .put("reportedAbis", JSONArray(Build.SUPPORTED_ABIS.toList()))
            .put("processArchitecture", System.getProperty("os.arch")).put("is64Bit", Process.is64Bit())
            .put("availableProcessors", memory.availableProcessors()).put("processCpuMs", Process.getElapsedCpuTime())
            .put("uptimeMs", SystemClock.uptimeMillis())
            .put("heapUsedBytes", memory.totalMemory() - memory.freeMemory()).put("heapMaxBytes", memory.maxMemory())
            .put("powerSaveMode", power.isPowerSaveMode).put("deviceIdleMode", power.isDeviceIdleMode)
            .put("interactive", power.isInteractive).put("ignoringBatteryOptimizations", power.isIgnoringBatteryOptimizations(context.packageName))
            .put("threads", cachedThreads)
        if (Build.VERSION.SDK_INT >= 29) android.put("thermalStatus", power.currentThermalStatus)
        if (Build.VERSION.SDK_INT >= 31) android.put("socManufacturer", Build.SOC_MANUFACTURER).put("socModel", Build.SOC_MODEL)
        val result = JSONObject().put("schema", 1).put("platform", "Android").put("time", System.currentTimeMillis() / 1000.0)
            .put("playback", playback).put("android", android).put("thermalState", android.optInt("thermalStatus", -1))
        for (key in listOf("appVersion", "appBuild", "volumeBalancePreference", "systemSpatialPreference", "playing", "paused",
            "background", "generation", "route", "engineRunning", "tracking", "motion", "backgroundTrackingEnabled", "error", "errorDetails", "input", "backgroundPlayback", "graphics", "accelerators")) {
            if (meta.has(key)) result.put(key, meta.get(key))
        }
        val missing = playback.optLong("callbackMissingFrames")
        val output = playback.optJSONObject("outputDevice")
        val hasXruns = output != null && output.has("hardwareXruns") && !output.isNull("hardwareXruns")
        val xruns = output?.optLong("hardwareXruns") ?: 0
        synchronized(lock) {
            if (serial != epoch || !enabled) return
            val same = generation == lastGeneration
            val point = JSONObject().put("time", result.getDouble("time")).put("generation", generation)
                .put("positionMs", playback.optLong("positionMs")).put("fifoFrames", playback.optLong("fifoFrames"))
                .put("pendingBatches", playback.optLong("pendingBatches"))
                .put("callbackMissingFramesDelta", if (same) (missing - previousMissing).coerceAtLeast(0) else 0)
                .put("hardwareXrunsDelta", if (!hasXruns) JSONObject.NULL else if (same) (xruns - previousXruns).coerceAtLeast(0) else 0)
                .put("renderMaxMicros", playback.optLong("renderMaxMicros"))
                .put("snapshotAgeMs", if (sample.elapsed > 0 && sample.generation == generation) now - sample.elapsed else JSONObject.NULL)
                .put("sampleIntervalMs", if (lastSampleAt > 0) now - lastSampleAt else 0)
                .put("processCpuMs", android.getLong("processCpuMs")).put("background", meta.optBoolean("background"))
                .put("paused", meta.optBoolean("paused")).put("thermalStatus", android.optInt("thermalStatus", -1))
                .put("sceneLoopRequested", meta.optJSONObject("graphics")?.optBoolean("sceneLoopRequested"))
            history.addLast(point); while (history.size > 60) history.removeFirst()
            result.put("history", JSONArray(history.toList())).put("events", JSONArray(events.toList()))
            result.put("nativeSnapshotAgeMs", point.get("snapshotAgeMs"))
            result.put("renderStageOrder", JSONArray(listOf("prepare", "fastMix", "fastReduce", "sample", "directional", "direct", "bus")))
            published = Published(result.toString(), now); lastSampleAt = now; lastGeneration = generation
            previousMissing = missing; previousXruns = xruns
        }
    }
    private fun threads(): JSONArray {
        val values = JSONArray()
        val dirs = File("/proc/self/task").listFiles() ?: return values
        for (dir in dirs) {
            if (values.length() >= 24) break
            try {
                val tid = dir.name.toInt()
                val name = File(dir, "comm").readText().trim()
                if (tid != Process.myPid() && !Regex("^(sda|SDA|AAudio|AudioTrack|RenderThread|mqt_js|JavaScript|js).*", RegexOption.IGNORE_CASE).matches(name)) continue
                val row = JSONObject().put("tid", tid).put("name", if (tid == Process.myPid()) "main" else name)
                val stat = File(dir, "stat").readText().substringAfterLast(") ").trim().split(Regex("\\s+"))
                row.put("state", stat[0]).put("nice", stat[16].toInt()).put("userTicks", stat[11].toLong()).put("systemTicks", stat[12].toLong())
                if (stat.size > 36) row.put("processor", stat[36].toInt())
                values.put(row)
            } catch (_: Exception) { }
        }
        return values
    }
    fun addresses(): List<String> = try {
        Collections.list(NetworkInterface.getNetworkInterfaces()).filter { it.isUp && !it.isLoopback }
            .flatMap { Collections.list(it.inetAddresses) }.filter { !it.isLoopbackAddress && it.address.size == 4 }
            .mapNotNull { it.hostAddress }.distinct()
    } catch (_: Exception) { emptyList() }

    private fun respond(socket: Socket, serial: Long) {
        val timeout = try { sampler?.schedule({ socket.runCatching { close() } }, 5, TimeUnit.SECONDS) } catch (_: Exception) { socket.close(); clients.remove(socket); return }
        try {
            socket.use {
                val deadline = SystemClock.elapsedRealtime() + 5_000
                val input = socket.getInputStream().buffered()
                val bytes = java.io.ByteArrayOutputStream()
                var tail = 0
                var complete = false
                val buffer = ByteArray(1024)
                while (bytes.size() < 8192 && !complete) {
                    val remaining = deadline - SystemClock.elapsedRealtime()
                    if (remaining <= 0 || serial != epoch || !enabled) return
                    socket.soTimeout = remaining.coerceAtMost(5_000).toInt().coerceAtLeast(1)
                    val count = input.read(buffer, 0, minOf(buffer.size, 8192 - bytes.size()))
                    if (count < 0) return
                    for (i in 0 until count) {
                        val next = buffer[i].toInt() and 255
                        bytes.write(next); tail = (tail shl 8) or next
                        if (tail == 0x0d0a0d0a) { complete = true; break }
                    }
                }
                if (!complete) return
                val parts = bytes.toString("UTF-8").substringBefore("\r\n").split(' ')
                val snapshot = published
                var code = "400 Bad Request"
                var body = "{}"
                if (parts.size == 3 && parts[2] in listOf("HTTP/1.0", "HTTP/1.1")) {
                    if (parts[0] != "GET") code = "405 Method Not Allowed"
                    else if (parts[1] !in listOf("/diagnostics", "/")) code = "404 Not Found"
                    else if (snapshot.json == "{}") code = "503 Service Unavailable"
                    else {
                        code = "200 OK"
                        body = JSONObject(snapshot.json).put("snapshotAgeMs", (SystemClock.elapsedRealtime() - snapshot.elapsed).coerceAtLeast(0)).toString()
                    }
                }
                val payload = body.toByteArray(Charsets.UTF_8)
                val header = "HTTP/1.1 $code\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: ${payload.size}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
                socket.getOutputStream().apply { write(header.toByteArray(Charsets.US_ASCII)); write(payload); flush() }
            }
        } catch (_: Exception) { }
        finally { timeout?.cancel(false); clients.remove(socket); socket.runCatching { close() } }
    }
}
