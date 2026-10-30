package app.sda.mobile.sda

import android.net.Uri
import com.sda.nativebridge.SdaEngine
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import org.json.JSONObject
import java.io.InputStream
import java.security.MessageDigest

/** Android Expo bridge for raw and M4A/MP4-contained E-AC-3/JOC streams. */
class SdaModule : Module() {
    private val mpeghImport = MpeghImport()
    private val stereoReferences = PreparedStereoReference()
    private val playbackQueue = NativePlaybackQueue()
    private val playbackCommands = java.util.concurrent.Executors.newSingleThreadExecutor { task -> Thread(task, "SDA native queue") }
    @Volatile private var preservePlaybackLease = false
    @Volatile private var handle: Long = 0L
    private var activeLayout = "7.1.4"
    private var pipelineHandle = 0L
    private var preparedHandle = 0L
    @Volatile private var activeIsMp3 = false
    @Volatile private var alacStereoUpmix = false
    @Volatile private var activeSourceCodec = ""
    private var feedThread: Thread? = null
    @Volatile private var backgroundSession = 0L
    @Volatile private var backgroundHash = ""
    @Volatile private var playbackYaw = 0.0
    @Volatile private var destroyed = false
    @Volatile
    private var stopped = false
    @Volatile
    private var feedError: String? = null
    @Volatile
    private var paused = false
    @Volatile
    private var feedDone = false
    @Volatile
    private var hrtfLoadStatus = "未加载"
    @Volatile
    private var activeInput: InputStream? = null
    @Volatile
    private var generation = 0L
    @Volatile private var diagnostics: RemoteDiagnostics? = null
    @Volatile private var inBackground = false
    @Volatile private var sceneRenderingRequested = false
    @Volatile private var failureCategory = ""
    @Volatile private var inputBytesSubmitted = 0L
    @Volatile private var dspCpuBudget = 0
    @Volatile private var audioCpuPolicy = JSONObject()
    private var audioPolicyMonitor: java.util.concurrent.ScheduledExecutorService? = null
    private fun startAudioPolicy(context: android.content.Context) {
        if (audioPolicyMonitor != null) return
        val power = context.getSystemService(android.os.PowerManager::class.java)
        val monitor = java.util.concurrent.Executors.newSingleThreadScheduledExecutor { task ->
            Thread(task, "SDA thermal policy").apply { priority = Thread.MIN_PRIORITY }
        }
        audioPolicyMonitor = monitor
        // getThermalHeadroom is sampled at most once per ten seconds. Unsupported
        // devices report NaN; measured render pressure remains available.
        monitor.scheduleWithFixedDelay({
            if (!destroyed) runCatching {
                val status = if (android.os.Build.VERSION.SDK_INT >= 29) power.currentThermalStatus else 0
                val headroom = if (android.os.Build.VERSION.SDK_INT >= 30) power.getThermalHeadroom(10) else Float.NaN
                SdaEngine.nativeUpdateAudioComputePressure(status, headroom)
                if (backgroundSession != 0L) dspCpuBudget = SdaEngine.nativeRefreshCpuBudget()
            }
        }, 0, 10, java.util.concurrent.TimeUnit.SECONDS)
    }
    private val nativeLock = Object()
    private val lifecycleLock = Object()

    private fun sha256(file: java.io.File): String {
        val digest = MessageDigest.getInstance("SHA-256")
        file.inputStream().use { input ->
            val buffer = ByteArray(16 * 1024)
            while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                digest.update(buffer, 0, count)
            }
        }
        return digest.digest().joinToString("") { "%02x".format(it) }
    }

    private fun spatialCueDb(): Double {
        val context = appContext.reactContext ?: error("no react context")
        val value = context.getSharedPreferences("sda-rendering", 0).getFloat("spatialCueDb", -6f).toDouble()
        return if (value in listOf(0.0, -3.0, -6.0, -9.0, -12.0)) value else -6.0
    }

    private fun restoreSpatialCues(ptr: Long, layout: String) {
        val db = spatialCueDb()
        // The packaged kernels already contain the shared default -6 dB cues.
        if (db == -6.0) return
        val context = appContext.reactContext ?: error("no react context")
        val path = java.io.File(context.filesDir, "hrtf-restored/hrtf-dense/hrtf-set.json").absolutePath
        val prepared = SdaEngine.nativePrepareSpatialCues(path, layout, Math.pow(10.0, db / 20.0).toFloat())
        check(prepared != 0L) { "空间线索准备失败" }
        try { val error = SdaEngine.nativeApplySpatialCues(ptr, prepared); check(error.isEmpty()) { error } }
        finally { SdaEngine.nativeFreeSpatialCues(prepared) }
    }

    private fun ensureEngine(layout: String = "7.1.4", startPipeline: Boolean = true): Long = synchronized(nativeLock) {
        if (handle != 0L) {
            if (startPipeline) startPreparedEngine(handle, activeLayout)
            return@synchronized handle
        }
        val context = appContext?.reactContext ?: throw RuntimeException("no react context")
        VendorNpuRuntime.prepare(context)
        // Ask Android to provision the app-owned directory (also used by
        // opt-in native PCM diagnostic builds), rather than creating it via adb.
        context.getExternalFilesDir(null)
        // One shared direct-only HRIR grid serves objects and bed channels.
        for (assetDirectory in listOf("hrtf-restored/hrtf-dense", "hrtf-restored/hrtf")) {
            val hrtfDir = java.io.File(context.filesDir, assetDirectory)
            check(hrtfDir.mkdirs() || hrtfDir.isDirectory) { "Cannot create KU100 asset directory: $hrtfDir" }
            val names = context.assets.list(assetDirectory)?.toList().orEmpty()
            val manifestName = "hrtf-set.json"
            val packedManifest = JSONObject(context.assets.open("$assetDirectory/$manifestName").bufferedReader().use { it.readText() })
            val packedPositions = packedManifest.getJSONArray("positions")
            val requiredResponses = mutableSetOf<String>()
            for (i in 0 until packedPositions.length()) {
                val row = packedPositions.getJSONObject(i)
                requiredResponses.add(row.getString("dry")); requiredResponses.add(row.getString("wet"))
            }
            if (packedManifest.has("triangleSupportFile")) requiredResponses.add(packedManifest.getString("triangleSupportFile"))
            check(manifestName in names && requiredResponses.all { it in names }) {
                "Packaged KU100 HRTF asset set is incomplete"
            }
            names.forEach { name ->
                val packaged = context.assets.open("$assetDirectory/$name").use { it.readBytes() }
                val destination = java.io.File(hrtfDir, name)
                val matches = if (name.endsWith(".f32")) {
                    val expected = MessageDigest.getInstance("SHA-256").digest(packaged)
                        .joinToString("") { "%02x".format(it) }
                    destination.isFile && destination.length() == packaged.size.toLong() && sha256(destination) == expected
                } else destination.isFile && destination.length() == packaged.size.toLong() &&
                    destination.readBytes().contentEquals(packaged)
                if (!matches) destination.writeBytes(packaged)
            }
            check(java.io.File(hrtfDir, manifestName).isFile) { "KU100 hrtf-set.json was not copied" }
        }
        hrtfLoadStatus = "空间渲染资源已就绪，正在加载"
        val settings = renderingSettings()
        val config = JSONObject().put("sampleRate", 48000).put("outputChannels", 2)
            .put("layout", layout).put("directObjectHrtf", settings.getBoolean("direct"))
            .put("directionalHrtf", settings.getBoolean("directional"))
            .put("binauralEnabled", settings.optBoolean("binauralEnabled", true))
            .put("objectRendering", settings.getBoolean("direct") || settings.getBoolean("directional"))
            .put("hrtfWetWeight", settings.getDouble("hrtfWetWeight")).toString()
        val assetDirectory = "hrtf-restored/hrtf-dense"
        val manifest = java.io.File(context.filesDir, "$assetDirectory/hrtf-set.json")
        val manifestInfo = JSONObject(manifest.readText())
        val directionCount = manifestInfo.getJSONArray("positions").length()
        val responseName = if (manifestInfo.optString("subjectId") == "apple-public-hrtfhq") "HRTFHQ" else "KU100 D1"
        val ptr = SdaEngine.nativeInit(config, manifest.absolutePath)
        if (ptr == 0L) {
            val detail = SdaEngine.nativeInitError()
            hrtfLoadStatus = "$responseName 加载失败: ${if (detail.isBlank()) "nativeInit failed" else detail}"
            throw RuntimeException(hrtfLoadStatus)
        }
        handle = ptr
        pipelineHandle = 0L
        activeLayout = layout
        try {
            val nearError = SdaEngine.nativeSetNearField(ptr, false, 1f)
            check(nearError.isEmpty()) { nearError }
        } catch (error: Throwable) {
            SdaEngine.nativeClose(ptr)
            handle = 0L
            throw error
        }
        val flavor = if (settings.getString("hrtfSet") == "dense-raw") "原始测量" else "校准资产"
        val wet = if (settings.getDouble("hrtfWetWeight") == 0.0) "无附加 HRTF 混响" else "HRTF 混响 ${settings.getDouble("hrtfWetWeight") }"
        hrtfLoadStatus = "已加载 $responseName · $directionCount 方向 · $flavor · $wet"
        if (startPipeline) startPreparedEngine(ptr, layout)
        ptr
    }

    private fun startPreparedEngine(ptr: Long, layout: String) {
        if (pipelineHandle == ptr) return
        val result = SdaEngine.nativeStart(ptr)
        if (result != 0) {
            val detail = SdaEngine.nativeLastError().ifBlank { "unknown native output error" }
            SdaEngine.nativeClose(ptr)
            handle = 0L
            throw RuntimeException("nativeStart failed: $detail ($result)")
        }
        pipelineHandle = ptr
        try {
            // nativeStart queues the requested layout on the render worker.
            // Cue application is acknowledged on that same queue, so a saved
            // 360RA-13 update cannot race the renderer's initial 7.1.4 layout.
            // No source PCM is submitted until ensureEngine returns.
            restoreSpatialCues(ptr, layout)
            val error = SdaEngine.nativeSetSpatialEnhancement(ptr, renderingSettings().getBoolean("spatialEnhancementEnabled"))
            check(error.isEmpty()) { error }
        } catch (error: Throwable) {
            SdaEngine.nativeClose(ptr); handle = 0L; pipelineHandle = 0L; throw error
        }
    }

    // Imported tracks may have a loaded renderer but no output pipeline yet.
    private fun discardIdleRenderer() {
        if (handle != 0L && pipelineHandle != handle) {
            SdaEngine.nativeClose(handle)
            handle = 0L; preparedHandle = 0L; pipelineHandle = 0L
        }
    }

    private fun stopFeedThread(keepSession: Boolean = false) {
        synchronized(lifecycleLock) {
            preservePlaybackLease = keepSession
            stopped = true
            try {
                activeInput?.close()
            } catch (_: Throwable) {
            }
            activeInput = null
            val worker = feedThread
            worker?.interrupt()
            try {
                if (worker != null) {
                    worker.join(2_000)
                    check(!worker.isAlive) { "Timed out stopping audio feed thread; native handle retained" }
                }
                feedThread = null
            } finally {
                // A timed-out/cancelled handoff must not retain the old service lease.
                preservePlaybackLease = false
            }
        }
    }

    private fun setPlaybackPaused(value: Boolean, token: Long = backgroundSession, fromFocus: Boolean = false): Boolean = synchronized(nativeLock) {
        if (handle == 0L || token == 0L || backgroundSession != token || !PlaybackKeepAlive.owns(token)) return@synchronized false
        if (fromFocus) {
            if (!PlaybackAudioFocus.shouldApply(token, !value)) return@synchronized false
        } else if (value) PlaybackAudioFocus.userPause(token)
        else if (!PlaybackAudioFocus.request(token)) return@synchronized false
        check(SdaEngine.nativePause(handle, value) == 0) { "native pause failed" }
        if (fromFocus && !value) PlaybackAudioFocus.didResume(token)
        paused = value
        diagnostics?.event(if (value) "pause" else "resume", generation)
        PlaybackKeepAlive.setPlaying(token, !value)
        sendEvent("onMediaCommand", mapOf("command" to "state", "hash" to backgroundHash, "paused" to value))
        true
    }

    private fun stopPlayback(token: Long = 0L): Boolean = playbackCommands.submit<Boolean> { stopPlaybackNow(if (token == 0L) backgroundSession else token) }.get()
    private fun stopPlaybackNow(token: Long): Boolean {
        synchronized(nativeLock) {
            if (token != 0L && backgroundSession != token) return false
            PlaybackKeepAlive.end(token)
            backgroundSession = 0L
        }
        stopFeedThread()
        synchronized(nativeLock) {
            if (handle != 0L) {
                SdaEngine.nativeFinish(handle)
                SdaEngine.nativeClose(handle)
                handle = 0L
            }
        }
        feedDone = true
        diagnostics?.event("stop", generation)
        sendEvent("onMediaCommand", mapOf("command" to "stop", "hash" to backgroundHash))
        return true
    }

    private fun diagnosticMetadata(context: android.content.Context): JSONObject {
        val prefs = context.getSharedPreferences("sda-rendering", 0)
        val info = context.packageManager.getPackageInfo(context.packageName, 0)
        val lease = PlaybackKeepAlive.diagnostics()
        return JSONObject().put("appVersion", info.versionName ?: "").put("appBuild", info.versionCode.toString())
            .put("volumeBalancePreference", prefs.getBoolean("volumeBalanceEnabled", false)).put("systemSpatialPreference", false)
            .put("playing", backgroundSession != 0L && handle != 0L && !feedDone && !stopped).put("paused", paused).put("background", inBackground)
            .put("generation", generation).put("route", "SDA-HRTFHQ").put("engineRunning", handle != 0L).put("tracking", "fixed-or-manual")
            .put("backgroundTrackingEnabled", false).put("motion", JSONObject())
            .put("error", if (failureCategory.isEmpty()) "" else "playback-error")
            .put("errorDetails", if (failureCategory.isEmpty()) JSONObject() else JSONObject().put("category", failureCategory))
            .put("input", JSONObject().put("codec", activeSourceCodec).put("layout", activeLayout)
                .put("bytesSubmitted", inputBytesSubmitted).put("dspCpuBudget", dspCpuBudget).put("audioCpuPolicy", audioCpuPolicy).put("feedThreadAlive", feedThread?.isAlive == true)
                .put("feedThreadState", feedThread?.state?.name ?: "NONE"))
            .put("backgroundPlayback", lease)
            .put("graphics", JSONObject().put("sceneLoopRequested", sceneRenderingRequested))
            .put("accelerators", JSONObject(SdaEngine.nativeAccelerators()))
    }
    private fun refreshAudioSchedulingSoon() {
        android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({
            if (!destroyed && backgroundSession != 0L) dspCpuBudget = SdaEngine.nativeRefreshCpuBudget()
        }, 100)
    }
    private fun recordFailure(error: Throwable) {
        val text = error.message.orEmpty()
        failureCategory = when {
            text.contains("disconnected", true) -> "audio-device-disconnected"
            text.contains("progress", true) -> "audio-output-no-progress"
            text.contains("drain", true) -> "audio-drain-timeout"
            text.contains("queue full", true) -> "native-command-queue-full"
            text.contains("nativeFeed", true) -> "native-feed-failed"
            else -> "playback-feed"
        }
        diagnostics?.event("error", generation)
    }

    // Caller holds nativeLock so the clock, identity and codec describe one track.
    private fun playbackStatus(): JSONObject = (if (handle != 0L) JSONObject(SdaEngine.nativeStatus(handle)) else JSONObject())
        .put("sourceCodec", activeSourceCodec)
        .put("outputLayout", if (activeSourceCodec == "alac" && !alacStereoUpmix) "2.0" else activeLayout)
        .put("alacUpmixActive", activeSourceCodec == "alac" && alacStereoUpmix).put("outputChannels", 2)
        .put("backgroundPlayback", PlaybackKeepAlive.diagnostics())

    private fun queueJson(): org.json.JSONArray = org.json.JSONArray().also { rows ->
        playbackQueue.snapshot().forEach { row -> rows.put(JSONObject().put("hash", row.hash).put("uri", row.uri)
            .put("name", row.name).put("originalName", row.originalName).put("metadata", JSONObject(row.metadata))) }
    }
    private fun configureQueue(text: String, mode: String, persist: Boolean = true) {
        val rows = org.json.JSONArray(text)
        val tracks = (0 until rows.length()).map { index ->
            val row = rows.getJSONObject(index)
            val hash = row.getString("hash")
            require(hash.matches(Regex("[0-9a-f]{64}"))) { "Invalid queue content hash" }
            NativeQueueTrack(hash, row.getString("uri"), row.getString("name"),
                (row.optJSONObject("metadata") ?: JSONObject()).toString(), row.optString("originalName", row.getString("name")))
        }
        playbackQueue.configure(tracks, mode)
        PlaybackKeepAlive.configureQueue(text)
        if (persist) appContext.reactContext?.getSharedPreferences("sda-playback-queue", 0)?.edit()
            ?.putString("rows", queueJson().toString())?.putString("mode", mode)?.apply()
    }
    private fun restoreQueue(context: android.content.Context) {
        val prefs = context.getSharedPreferences("sda-playback-queue", 0)
        runCatching { configureQueue(prefs.getString("rows", "[]") ?: "[]", prefs.getString("mode", "sequence") ?: "sequence", false) }
        backgroundHash = prefs.getString("selected", "") ?: ""
    }
    private fun requestQueueAdvance(hash: String, token: Long, automatic: Boolean, direction: Int = 1): Boolean {
        if (destroyed || !PlaybackKeepAlive.owns(token) || playbackQueue.next(hash, automatic, direction) == null) return false
        return try {
            playbackCommands.execute {
                // A queued duplicate cannot skip the replacement song.
                if (destroyed || backgroundSession != token || backgroundHash != hash || !PlaybackKeepAlive.owns(token)) return@execute
                val next = playbackQueue.next(hash, automatic, direction)
                if (next == null) {
                    appContext.reactContext?.let { PlaybackKeepAlive.finish(it, token) }
                    if (backgroundSession == token) backgroundSession = 0L
                    return@execute
                }
                try {
                    startPlayback(next.uri, next.name, playbackYaw, next.hash)
                    PlaybackKeepAlive.setMetadata(next.hash, next.metadata)
                    diagnostics?.event("nativeQueueAdvance", generation)
                } catch (error: Throwable) {
                    recordFailure(error); feedError = error.message ?: error.toString(); feedDone = true
                    PlaybackKeepAlive.end(backgroundSession); backgroundSession = 0L
                }
            }
            true
        } catch (_: java.util.concurrent.RejectedExecutionException) { false }
    }

    private fun startPlayback(uriString: String, displayName: String, headYawDegrees: Double, contentHash: String): String {
            require(headYawDegrees.isFinite() && headYawDegrees in -180.0..180.0) { "Head yaw must be finite and between -180 and 180 degrees" }
            require(contentHash.matches(Regex("[0-9a-f]{64}"))) { "Invalid track content hash" }
            val targetLayout = if (displayName.endsWith(".mhas", true)) "360RA-13" else "7.1.4"
            val reusePrepared = synchronized(nativeLock) { handle != 0L && handle == preparedHandle && backgroundSession == 0L && activeLayout == targetLayout }
            stopFeedThread(keepSession = true)
            failureCategory = ""; inputBytesSubmitted = 0L
            val context = appContext?.reactContext ?: throw RuntimeException("no react context")
            val uri = Uri.parse(uriString)
            synchronized(nativeLock) {
                if (handle != 0L && !reusePrepared) {
                    SdaEngine.nativeFinish(handle)
                    SdaEngine.nativeClose(handle)
                    handle = 0L
                }
            }
            backgroundSession = 0L
            preparedHandle = 0L
            var mediaToken = 0L
            val mediaProgress = java.util.concurrent.atomic.AtomicReference(PlaybackKeepAlive.Progress(0, true))
            val controls = object : PlaybackKeepAlive.Controls {
                override fun setPlaying(playing: Boolean) = setPlaybackPaused(!playing, mediaToken)
                override fun setInterrupted(playing: Boolean) = setPlaybackPaused(!playing, mediaToken, fromFocus = true)
                override fun stop() { stopPlayback(mediaToken) }
                override fun skip(direction: Int) {
                    requestQueueAdvance(contentHash, mediaToken, automatic = false, direction = direction)
                }
                override fun progress(): PlaybackKeepAlive.Progress? {
                    // Quota/cpuset discovery can read system files. Refresh on
                    // this existing 1 Hz control executor, outside the decoder lock.
                    dspCpuBudget = SdaEngine.nativeRefreshCpuBudget()
                    if (backgroundSession != mediaToken || handle == 0L || !PlaybackKeepAlive.owns(mediaToken)) return null
                    return mediaProgress.get()
                }
            }
            val playbackSession = synchronized(nativeLock) {
                check(!destroyed) { "Audio module was destroyed" }
                backgroundHash = contentHash
                context.getSharedPreferences("sda-playback-queue", 0).edit().putString("selected", contentHash).apply()
                PlaybackKeepAlive.begin(context, displayName, contentHash, controls).also {
                    mediaToken = it; backgroundSession = it
                }
            }
            return try {
            // Versioned full-content identity survives temporary MP4 -> MHAS
            // imports and cannot reuse a result for a changed source file.
            val referenceVersion = if (displayName.substringAfterLast('.', "").equals("mhas", true)) "v7-mpegh-prelimit" else "v6"
            val cacheKey = "sda-measured-lufs-$referenceVersion:$contentHash:48000"
            val loudnessCache = context.getSharedPreferences("sda-loudness", 0)
            val isAv3a = displayName.substringAfterLast('.', "").equals("av3a", ignoreCase = true)
            val isMpegh = displayName.substringAfterLast('.', "").equals("mhas", ignoreCase = true)
            val isMp3 = displayName.substringAfterLast('.', "").equals("mp3", ignoreCase = true)
            val isContainer = displayName.substringAfterLast('.', "").lowercase() in setOf("m4a", "mp4")
            val isWavName = displayName.substringAfterLast('.', "").equals("wav", ignoreCase = true)
            val isSdarName = displayName.substringAfterLast('.', "").equals("sdar", ignoreCase = true)
            // DocumentPicker normally supplies a local file. Reuse it rather
            // than copying the full song again just to probe its container.
            val localFile = if (uri.scheme == "file") uri.path?.let { java.io.File(it) }?.takeIf { it.isFile } else null
            val ownsSourceCopy = (isMp3 || isContainer || isSdarName || isWavName) && localFile == null
            val mp3Cache = if (isMp3 || isContainer || isSdarName) localFile ?: java.io.File.createTempFile("sda-seekable-", if (isMp3) ".mp3" else ".m4a", context.cacheDir) else null
            if (ownsSourceCopy && mp3Cache != null) {
                try {
                    val source = context.contentResolver.openInputStream(uri) ?: error("无法打开所选音频文件")
                    source.use { stream -> mp3Cache.outputStream().use { stream.copyTo(it, 64 * 1024) } }
                } catch (error: Throwable) {
                    mp3Cache.delete()
                    throw error
                }
            }
            val isSdar = mp3Cache != null && SdaEngine.nativeIsSdar(mp3Cache.absolutePath)
            if(isSdarName && !isSdar){if(ownsSourceCopy)mp3Cache?.delete();error("无效或尚未支持的 SDAR 床层文件")}
            if(isWavName && !isSdar){if(ownsSourceCopy)mp3Cache?.delete();error("WAV 文件缺少 SDA 母版渲染元数据")}
            val isAlac = !isSdar && isContainer && mp3Cache != null && SdaEngine.nativeIsAlac(mp3Cache.absolutePath)
            val input = try {
                if (isMp3 || isAlac || isSdar) java.io.ByteArrayInputStream(ByteArray(0)) else Eac3Input.open(context, uri, displayName, if (isContainer) mp3Cache else null)
            } catch (error: Throwable) {
                if (ownsSourceCopy) mp3Cache?.delete()
                throw error
            }
            val ptr = try {
                synchronized(nativeLock) {
                check(!destroyed && PlaybackKeepAlive.owns(playbackSession)) { "Playback preparation was cancelled" }
                ensureEngine(if (isMpegh) "360RA-13" else "7.1.4").also {
                    if (isMpegh) check(SdaEngine.nativeOpenMpegh(it) == 0) {
                        "360RA 打开失败: ${SdaEngine.nativeLastError()}"
                    }
                    if (isSdar && mp3Cache != null) check(SdaEngine.nativeOpenSdar(it, mp3Cache.absolutePath, context.cacheDir.absolutePath) > 0) {
                        "SDAR 打开失败: ${SdaEngine.nativeLastError()}"
                    }
                    if (isAlac && mp3Cache != null) check(SdaEngine.nativeOpenAlac(it, mp3Cache.absolutePath) > 0) {
                        "ALAC 打开失败: ${SdaEngine.nativeLastError()}"
                    }
                    if (isMp3 && mp3Cache != null) check(SdaEngine.nativeOpenMp3(it, mp3Cache.absolutePath) > 0) {
                        "MP3 打开失败: ${SdaEngine.nativeLastError()}"
                    }
                    val balanceError = SdaEngine.nativeSetVolumeBalance(it, renderingSettings().getBoolean("volumeBalanceEnabled"))
                    check(balanceError.isEmpty()) { balanceError }
                    if (!isAlac && !isMp3 && !isAv3a && !isSdar) loudnessCache.getString(cacheKey, null)?.let { cached ->
                        // A damaged/stale cache is a miss, never a playback failure.
                        val valid = runCatching {
                            val m = JSONObject(cached)
                            m.getDouble("integratedLufs").isFinite() && m.getInt("blocks") >= 57
                        }.getOrDefault(false)
                        if (valid) SdaEngine.nativeSetMeasuredLoudness(it, cached)
                        else loudnessCache.edit().remove(cacheKey).apply()
                    }
                    check(SdaEngine.nativeSetHeadYaw(it, headYawDegrees.toFloat()) == 0) {
                        "Native head yaw command failed during startup"
                    }
                    playbackYaw = headYawDegrees
                }
                }
            } catch (error: Throwable) {
                input.close()
                if (ownsSourceCopy) mp3Cache?.delete()
                synchronized(nativeLock) {
                    if (handle != 0L) {
                        SdaEngine.nativeClose(handle)
                        handle = 0L
                    }
                }
                throw error
            }
            val workerGeneration = synchronized(lifecycleLock) {
                generation += 1
                diagnostics?.event("play", generation)
                generation
            }
            feedError = null
            feedDone = false
            paused = false
            stopped = false
            activeIsMp3 = isMp3
            alacStereoUpmix = renderingSettings().getBoolean("alacStereoUpmix")
            // Container AC-4 was normalized by the shared importer before opening.
            activeSourceCodec = if (isSdar) "sdar" else if (isAlac) "alac" else if (isMpegh) "mpegh" else if (isMp3) "mp3"
                else if (displayName.substringAfterLast('.', "").equals("ac4", ignoreCase = true)) "ac4"
                else if (displayName.substringAfterLast('.', "").equals("av3a", ignoreCase = true)) "av3a" else "eac3"
            activeInput = input
            synchronized(nativeLock) {
                check(!destroyed && handle == ptr && backgroundSession == playbackSession && PlaybackKeepAlive.owns(playbackSession)) { "Playback preparation was cancelled" }
                paused = !PlaybackAudioFocus.request(playbackSession)
                check(SdaEngine.nativePause(ptr, paused) == 0) { "native startup pause failed" }
                PlaybackKeepAlive.setPlaying(playbackSession, !paused)
            }
            val worker = Thread({
                try {
                    try { android.os.Process.setThreadPriority(-4) } catch (_: SecurityException) { }
                    audioCpuPolicy = JSONObject(SdaEngine.nativePrepareDecoderThread())

                    // Imported sources have a fresh, memory-only reference already.
                    // Restored/changed files still take a complete fallback measurement.
                    if (!isSdar && (isAlac || isMp3)) {
                        val file = mp3Cache ?: error("立体声分析输入缺失")
                        val measurement = stereoReferences.ready(file, contentHash) ?: stereoReferences.prepare(file, contentHash, isAlac) {
                            stopped || destroyed || generation != workerGeneration || handle != ptr || !PlaybackKeepAlive.owns(playbackSession)
                        }
                        synchronized(nativeLock) {
                            if (stopped || destroyed || generation != workerGeneration || handle != ptr || !PlaybackKeepAlive.owns(playbackSession)) return@Thread
                            val error = SdaEngine.nativeSetMeasuredLoudness(ptr, measurement)
                            check(error.isEmpty()) { error }
                        }
                    }

                    input.use { stream ->
                        val buffer = ByteArray(4 * 1024)
                        var lastConsumed = 0L
                        var lastProgressNs = System.nanoTime()
                        // player.ts TARGET_AHEAD_SECONDS: same four-second decode
                        // reserve, paced against presented audio rather than FIFO size.
                        val maxLead = if (isAlac) 24_000L else 4 * 48_000L
                        while (!stopped && !destroyed && PlaybackKeepAlive.owns(playbackSession)) {
                            val state = synchronized(nativeLock) {
                                if (generation != workerGeneration || handle != ptr || stopped) return@Thread
                                JSONObject(SdaEngine.nativeStatus(ptr))
                            }
                            mediaProgress.set(PlaybackKeepAlive.Progress(state.optLong("positionMs"), state.optBoolean("preparingAudio")))
                            diagnostics?.observeNative(state, workerGeneration)
                            val decoded = state.getLong("decodedSamplePos")
                            val consumed = state.getLong("consumedSamplePos")
                            val fifoFrames = state.optInt("fifoFrames", 0)
                            if (paused || state.optBoolean("paused", false) || state.optJSONObject("outputDevice")?.optBoolean("recovering", false) == true) {
                                lastProgressNs = System.nanoTime()
                                Thread.sleep(20)
                                continue
                            }
                            check(state.optJSONObject("outputDevice")?.optInt("errorCode", 0)?.let { it >= 0 } != false) {
                                "Android audio device disconnected"
                            }
                            val preparingBalance = state.optBoolean("preparingAudio", false)
                            if (consumed != lastConsumed || preparingBalance) {
                                lastConsumed = consumed
                                lastProgressNs = System.nanoTime()
                            }
                            check(decoded == 0L || System.nanoTime() - lastProgressNs < 15_000_000_000L) {
                                "Audio consumption stalled (decoded=$decoded consumed=$consumed fifo=$fifoFrames " +
                                    "callbacks=${state.optLong("outputCallbacks")} renderBlocks=${state.optLong("renderBlocks")} " +
                                    "backend=${state.optJSONObject("outputDevice")?.optString("backend")})"
                            }
                            if (!preparingBalance && (decoded - consumed > maxLead || fifoFrames > maxLead)) {
                                Thread.sleep(20)
                                continue
                            }
                            if (isMp3 || isAlac || isSdar) {
                                val pulled = synchronized(nativeLock) {
                                    if (generation != workerGeneration || handle != ptr || stopped) return@Thread
                                    if (isSdar) SdaEngine.nativePullSdar(ptr,4096) else if (isAlac) SdaEngine.nativePullAlac(ptr, 4096, alacStereoUpmix) else SdaEngine.nativePullMp3(ptr, 4096)
                                }
                                if (pulled == -4) break
                                check(pulled >= 0) { "音频解码失败: ${SdaEngine.nativeLastError()}" }
                                continue
                            }
                            val count = stream.read(buffer)
                            if (count < 0) break
                            if (count == 0) continue
                            val result = synchronized(nativeLock) {
                                if (generation != workerGeneration || handle != ptr || stopped) -1 else SdaEngine.nativeFeed(
                                    ptr,
                                    buffer.copyOf(count)
                                )
                            }
                            if (result >= 0) inputBytesSubmitted += count
                            check(result >= 0) { "nativeFeed failed: ${SdaEngine.nativeLastError()}" }
                        }
                        if (!stopped && !destroyed && PlaybackKeepAlive.owns(playbackSession)) {
                            var eofDeadline = System.nanoTime() + 15_000_000_000L
                            synchronized(nativeLock) {
                                if (generation == workerGeneration && handle == ptr && !stopped) {
                                    val result = SdaEngine.nativeFinish(ptr)
                                    check(result >= 0) { "nativeFinish failed: ${SdaEngine.nativeLastError()}" }
                                    // Match Windows: save only successfully flushed full
                                    // decodes, not a stopped intro or a failed feed.
                                    val measurement = SdaEngine.nativeCompleteLoudness(ptr)
                                    if (!isAlac && !isMp3 && !isAv3a && !isSdar && measurement != "null") loudnessCache.edit().putString(cacheKey, measurement).apply()
                                }
                            }
                            while (!stopped && !destroyed && PlaybackKeepAlive.owns(playbackSession)) {
                                val state = synchronized(nativeLock) {
                                    if (generation != workerGeneration || handle != ptr || stopped) return@Thread
                                    JSONObject(SdaEngine.nativeStatus(ptr))
                                }
                                mediaProgress.set(PlaybackKeepAlive.Progress(state.optLong("positionMs"), state.optBoolean("preparingAudio")))
                            diagnostics?.observeNative(state, workerGeneration)
                                val decoded = state.getLong("decodedSamplePos")
                                val consumed = state.getLong("consumedSamplePos")
                                val remaining = if (decoded > consumed) decoded - consumed else 0L
                                if (remaining == 0L) break
                                if (paused) {
                                    eofDeadline = System.nanoTime() + 15_000_000_000L
                                    Thread.sleep(20)
                                    continue
                                }
                                check(System.nanoTime() < eofDeadline) { "Timed out waiting for audio drain at EOF" }
                                Thread.sleep(20)
                            }
                            synchronized(nativeLock) {
                                if (generation == workerGeneration && handle == ptr && !stopped) {
                                    SdaEngine.nativeClose(ptr)
                                    handle = 0L
                                    feedDone = true
                                    diagnostics?.event("ended", workerGeneration)
                                }
                            }
                        }
                    }
                } catch (error: InterruptedException) {
                    Thread.currentThread().interrupt()
                } catch (error: Throwable) {
                    synchronized(nativeLock) {
                        // A cancelled analysis must not publish an old failure to the next source.
                        if (generation == workerGeneration && backgroundSession == playbackSession && !stopped && !destroyed) {
                            recordFailure(error)
                            feedError = error.message ?: error.toString()
                            if (handle == ptr) {
                                SdaEngine.nativeClose(ptr)
                                handle = 0L
                            }
                            feedDone = true
                        }
                    }
                } finally {
                    val handingOff = if (feedDone && feedError == null && !stopped)
                        requestQueueAdvance(contentHash, playbackSession, automatic = true) else stopped && preservePlaybackLease
                    if (!handingOff) {
                        if (feedDone && feedError == null && !stopped) PlaybackKeepAlive.finish(context, playbackSession)
                        else PlaybackKeepAlive.end(context, playbackSession)
                        if (backgroundSession == playbackSession) backgroundSession = 0L
                    }
                    if (ownsSourceCopy) mp3Cache?.delete()
                }
            }, "sda-content-feed")
            feedThread = worker
            worker.start()
            displayName
            } catch (error: Throwable) {
                recordFailure(error)
                PlaybackKeepAlive.end(context, playbackSession)
                if (backgroundSession == playbackSession) backgroundSession = 0L
                throw error
            }
    }

    override fun definition() = ModuleDefinition {
        OnCreate { appContext.reactContext?.let { restoreQueue(it); VendorNpuRuntime.prepare(it); startAudioPolicy(it.applicationContext) } }
        Name("SdaEngine")
        Events("onMediaCommand")
        OnActivityEntersForeground { inBackground = false; refreshAudioSchedulingSoon(); diagnostics?.event("foreground", generation) }
        OnActivityEntersBackground { inBackground = true; refreshAudioSchedulingSoon(); diagnostics?.event("background", generation) }
        Function("configureDiagnostics") { enabled: Boolean ->
            val context = appContext.reactContext ?: error("no react context")
            val service = diagnostics ?: RemoteDiagnostics(context.applicationContext).also { diagnostics = it }
            service.configure(enabled) { diagnosticMetadata(context.applicationContext) }
        }
        Function("setSceneRenderingRequested") { requested: Boolean ->
            if (sceneRenderingRequested != requested) {
                sceneRenderingRequested = requested
                diagnostics?.event(if (requested) "scene-render-start" else "scene-render-stop", generation)
            }
        }
        Function("diagnosticsStatus") { diagnostics?.status ?: "已关闭" }
        Function("diagnosticsEnabled") { diagnostics?.isEnabled == true }
        Function("diagnosticsAddresses") { diagnostics?.addresses()?.joinToString(" · ") ?: "" }
        Function("configurePlaybackQueue") { text: String, mode: String -> configureQueue(text, mode) }
        Function("playbackQueueSnapshot") { JSONObject().put("queue", queueJson()).put("currentHash", backgroundHash)
            .put("mode", playbackQueue.playbackMode()).toString() }
        Function("skipPlayback") { direction: Int -> require(direction == -1 || direction == 1); requestQueueAdvance(backgroundHash, backgroundSession, false, direction) }
        Function("playbackSnapshot") { includeObjects: Boolean -> synchronized(nativeLock) {
            val status = playbackStatus()
            JSONObject().put("generation", generation).put("trackHash", backgroundHash)
                .put("hasPlayback", backgroundSession != 0L && !feedDone).put("feedDone", feedDone)
                .put("feedError", feedError ?: JSONObject.NULL).put("durationMs", PlaybackKeepAlive.snapshot()?.durationMs ?: 0L)
                .put("status", status).put("objects", if (includeObjects && handle != 0L) JSONObject(SdaEngine.nativeObjects(handle)) else JSONObject())
                .put("hrtfStatus", hrtfLoadStatus).toString()
        } }
        AsyncFunction("retainQueueTrack") { uri: String, name: String, hash: String ->
            require(hash.matches(Regex("[0-9a-f]{64}")))
            val context = appContext.reactContext ?: error("no react context")
            val source = Uri.parse(uri).path?.let { java.io.File(it).canonicalFile } ?: error("请选择本地文件")
            check(source.isFile) { "音频文件已不存在，请重新打开" }
            val ext = name.substringAfterLast('.', "audio").lowercase().filter { it.isLetterOrDigit() }.take(8)
            val directory = java.io.File(context.filesDir, "playback-library").apply { mkdirs() }
            val target = java.io.File(directory, "$hash.$ext")
            if (source != target.canonicalFile) {
                val owned = source.toPath().startsWith(context.cacheDir.canonicalFile.toPath())
                if (!owned || !source.renameTo(target)) { source.copyTo(target, overwrite = true); if (owned) source.delete() }
            }
            Uri.fromFile(target).toString()
        }
        AsyncFunction("prepareStereoTrack") { uri: String, name: String, hash: String ->
            require(hash.matches(Regex("[0-9a-f]{64}")))
            val ext = name.substringAfterLast('.', "").lowercase()
            if (ext in setOf("mp3", "m4a", "mp4")) {
                val file = Uri.parse(uri).path?.let { java.io.File(it) } ?: error("请选择本地文件")
                if (!SdaEngine.nativeIsSdar(file.absolutePath)) {
                    val alac = ext != "mp3" && SdaEngine.nativeIsAlac(file.absolutePath)
                    if (alac || ext == "mp3") stereoReferences.prepare(file, hash, alac) { destroyed || Thread.currentThread().isInterrupted }
                }
            }
            true
        }
        Function("setNowPlayingMetadata") { hash: String, text: String -> PlaybackKeepAlive.setMetadata(hash, text) }

        AsyncFunction("preparePlayback") { displayName: String ->
            synchronized(nativeLock) {
                check(!destroyed) { "Audio module was destroyed" }
                if (backgroundSession == 0L && handle == 0L) {
                    preparedHandle = ensureEngine(if (displayName.endsWith(".mhas", true)) "360RA-13" else "7.1.4", startPipeline = false)
                }
            }
        }
        AsyncFunction("adoptMp4Import") { token: String ->
            val context = appContext.reactContext ?: error("no react context")
            mpeghImport.adopt(context, token)
        }
        AsyncFunction("discardPickerCopy") { uri: String ->
            val context = appContext.reactContext ?: error("no react context")
            MpeghImport.discardPickerCopy(context, Uri.parse(uri))
        }
        AsyncFunction("contentHash") { uriString: String ->
            val context = appContext.reactContext ?: throw RuntimeException("no react context")
            MediaMetadata.contentHash(context, Uri.parse(uriString))
        }

        AsyncFunction("metadata") { uriString: String ->
            val context = appContext.reactContext ?: throw RuntimeException("no react context")
            val uri=Uri.parse(uriString)
            val file=if(uri.scheme=="file") uri.path?.let { java.io.File(it) } else null
            if(file!=null && file.isFile && SdaEngine.nativeIsSdar(file.absolutePath)) {
                val metadata=JSONObject(SdaEngine.nativeSdarMetadata(file.absolutePath))
                check(!metadata.has("error")) { metadata.optString("error") }
                metadata.toString()
            } else MediaMetadata.read(context, uri).toString()
        }

        AsyncFunction("durationMs") { uriString: String ->
            val context = appContext.reactContext ?: throw RuntimeException("no react context")
            Eac3Input.durationMs(context, Uri.parse(uriString))
        }

        Function("setVolumeBalance") { enabled: Boolean ->
            synchronized(nativeLock) {
                if (handle != 0L && pipelineHandle == handle) {
                    val error = SdaEngine.nativeSetVolumeBalance(handle, enabled)
                    check(error.isEmpty()) { error }
                }
                val context = appContext.reactContext ?: error("no react context")
                context.getSharedPreferences("sda-rendering", 0).edit()
                    .putBoolean("volumeBalanceEnabled", enabled).apply()
            }
        }

        Function("setAlacStereoUpmix") { enabled: Boolean ->
            synchronized(nativeLock) {
                val context = appContext.reactContext ?: error("no react context")
                check(context.getSharedPreferences("sda-rendering", 0).edit()
                    .putBoolean("alacStereoUpmix", enabled).commit()) { "无法保存 ALAC 上混" }
                alacStereoUpmix = enabled
            }
        }

        Function("setSpatialEnhancement") { enabled: Boolean ->
            synchronized(nativeLock) {
                if (handle != 0L && pipelineHandle == handle) {
                    val error = SdaEngine.nativeSetSpatialEnhancement(handle, enabled)
                    check(error.isEmpty()) { error }
                }
                val context = appContext.reactContext ?: error("no react context")
                check(context.getSharedPreferences("sda-rendering", 0).edit()
                    .putBoolean("spatialEnhancementEnabled", enabled).commit()) { "无法保存空间平衡设置" }
            }
        }

        AsyncFunction("setSpatialCueDb") { db: Double ->
            require(db in listOf(0.0, -3.0, -6.0, -9.0, -12.0)) { "空间线索强度无效" }
            val context = appContext.reactContext ?: error("no react context")
            val preferences = context.getSharedPreferences("sda-rendering", 0)
            val snapshot = synchronized(nativeLock) {
                discardIdleRenderer()
                if (handle == 0L) {
                    check(preferences.edit().putFloat("spatialCueDb", db.toFloat()).commit()) { "无法保存空间线索设置" }
                    null
                } else Triple(handle, generation, activeLayout)
            }
            if (snapshot != null) {
                val path = java.io.File(context.filesDir, "hrtf-restored/hrtf-dense/hrtf-set.json").absolutePath
                // Disk IO / FFT preparation must not hold the decoder/feed lock.
                val prepared = SdaEngine.nativePrepareSpatialCues(path, snapshot.third, Math.pow(10.0, db / 20.0).toFloat())
                check(prepared != 0L) { "空间线索准备失败" }
                try {
                    synchronized(nativeLock) {
                        check(handle == snapshot.first && generation == snapshot.second && activeLayout == snapshot.third) {
                            "播放曲目已切换，请重新选择空间线索强度"
                        }
                        val error = SdaEngine.nativeApplySpatialCues(handle, prepared)
                        check(error.isEmpty()) { error }
                        check(preferences.edit().putFloat("spatialCueDb", db.toFloat()).commit()) { "无法保存空间线索设置" }
                    }
                } finally { SdaEngine.nativeFreeSpatialCues(prepared) }
            }
        }

        Function("renderingSettings") { -> renderingSettings().toString() }

        // Keep the live decoder and audio sink; acknowledge the new graph before saving.
        AsyncFunction("setRenderingPreset") { id: String ->
            val context = appContext.reactContext ?: error("no react context")
            val profiles = org.json.JSONArray(context.assets.open("rendering-presets.json").bufferedReader().use { it.readText() })
            val profile = (0 until profiles.length()).map { profiles.getJSONObject(it) }
                .firstOrNull { it.getString("id") == id } ?: error("未知空间渲染预设")
            val set = profile.getString("hrtfSet")
            require(set == "dense") { "无效 HRTF 预设" }
            val wetWeight = profile.getDouble("hrtfWetWeight")
            require(wetWeight.isFinite() && wetWeight in 0.0..1.0) { "无效 HRTF 混响权重" }
            synchronized(nativeLock) {
                discardIdleRenderer()
                require(!profile.getBoolean("nearField") && profile.getString("roomId").isEmpty()) { "不支持的预设房间配置" }
                if (handle != 0L) {
                    val directory = "hrtf-restored/hrtf-dense"
                    val manifest = java.io.File(context.filesDir, "$directory/hrtf-set.json")
                    val error = SdaEngine.nativeSetHrtfPreset(handle, manifest.absolutePath,
                        wetWeight.toFloat(), profile.getBoolean("direct"), profile.getBoolean("directional"))
                    check(error.isEmpty()) { error }
                    restoreSpatialCues(handle, activeLayout)
                }
                check(context.getSharedPreferences("sda-rendering", 0).edit()
                    .putString("hrtfSet", set)
                    .putFloat("hrtfWetWeight", wetWeight.toFloat())
                    .putBoolean("direct", profile.getBoolean("direct"))
                    .putBoolean("directional", profile.getBoolean("directional"))
                    .putBoolean("nearField", profile.getBoolean("nearField"))
                    .putString("roomId", profile.getString("roomId")).commit()) { "无法保存空间渲染预设" }
                hrtfLoadStatus = "${profile.getString("label")} · ${if (handle != 0L) "已加载" else "等待播放加载"}"
            }
        }

        AsyncFunction("setNearField") { enabled: Boolean, metresPerUnit: Double ->
            require(!enabled) { "移动端已移除近场渲染" }
            require(metresPerUnit.isFinite() && metresPerUnit in 0.25..4.0) { "近场距离映射必须在 0.25–4 米之间" }
            synchronized(nativeLock) {
                if (handle != 0L) {
                    val error = SdaEngine.nativeSetNearField(handle, enabled, metresPerUnit.toFloat())
                    check(error.isEmpty()) { error }
                }
                val context = appContext.reactContext ?: error("no react context")
                context.getSharedPreferences("sda-rendering", 0).edit()
                    .putBoolean("nearField", enabled).putFloat("metresPerUnit", metresPerUnit.toFloat()).apply()
            }
        }

        Function("rooms") { -> "[]" }

        AsyncFunction("setRoom") { id: String ->
            val context = appContext.reactContext ?: error("no react context")
            require(id.isEmpty()) { "移动端已移除房间仿真" }
            val path = ""
            synchronized(nativeLock) {
                discardIdleRenderer()
                if (handle != 0L) {
                    val error = SdaEngine.nativeSetRoom(handle, path)
                    check(error.isEmpty()) { error }
                }
                val preferences = context.getSharedPreferences("sda-rendering", 0).edit().putString("roomId", id)
                preferences.apply()
            }
        }

        Function("setBinauralEnabled") { enabled: Boolean ->
            synchronized(nativeLock) {
                if (handle != 0L) check(SdaEngine.nativeSetBinauralEnabled(handle, enabled) == 0) { "空间音频设置未被原生引擎接受" }
                val context = appContext.reactContext ?: error("no react context")
                context.getSharedPreferences("sda-rendering", 0).edit().putBoolean("binauralEnabled", enabled).apply()
            }
        }

        Function("setObjectRendering") { direct: Boolean, directional: Boolean ->
            synchronized(nativeLock) {
                discardIdleRenderer()
                if (handle != 0L) {
                    check(SdaEngine.nativeSetObjectRendering(handle, direct, directional) == 0) {
                        "对象渲染设置未被原生引擎接受"
                    }
                }
                val context = appContext.reactContext ?: throw RuntimeException("no react context")
                context.getSharedPreferences("sda-rendering", 0).edit()
                    .putBoolean("direct", direct).putBoolean("directional", directional).apply()
            }
        }

        AsyncFunction("beginMp4Import") { uri: String ->
            mpeghImport.begin(appContext.reactContext ?: error("no react context"), Uri.parse(uri))
        }
        AsyncFunction("readMp4Import") { token: String, offset: Double, count: Int -> mpeghImport.read(token, offset, count) }
        AsyncFunction("appendMp4Import") { token: String, bytes: String -> mpeghImport.append(token, bytes) }
        AsyncFunction("finishMp4Import") { token: String -> mpeghImport.finish(token) }
        AsyncFunction("discardMp4Import") { token: String -> mpeghImport.discard(token) }

        AsyncFunction("playUri") { uriString: String, displayName: String, headYawDegrees: Double, contentHash: String ->
            playbackCommands.submit<String> { startPlayback(uriString, displayName, headYawDegrees, contentHash) }.get()
        }


        Function("pause") { -> setPlaybackPaused(true) }
        Function("resume") { -> setPlaybackPaused(false) }
        Function("stop") { -> stopPlayback() }

        Function("status") { -> synchronized(nativeLock) { playbackStatus().toString() } }

        Function("objects") { ->
            synchronized(nativeLock) {
                if (handle == 0L) "{}" else SdaEngine.nativeObjects(handle)
            }
        }

        Function("setHeadYaw") { degrees: Double ->
            require(degrees.isFinite() && degrees in -180.0..180.0) { "Head yaw must be finite and between -180 and 180 degrees" }
            synchronized(nativeLock) {
                check(
                    handle != 0L && SdaEngine.nativeSetHeadYaw(
                        handle,
                        degrees.toFloat()
                    ) == 0
                ) { "Native head yaw command failed" }
                playbackYaw = degrees
            }
        }

        Function("resetHeadPose") { ->
            synchronized(nativeLock) {
                check(handle != 0L && SdaEngine.nativeResetHeadPose(handle) == 0) { "Native head pose reset failed" }
                playbackYaw = 0.0
            }
        }

        Function("hrtfStatus") { -> hrtfLoadStatus }

        Function("feedError") { -> feedError }

        Function("feedDone") { -> feedDone }

        Function("stereoBedMode") { -> activeIsMp3 }
        Function("nativeLastError") { -> SdaEngine.nativeLastError() }

        Function("setVolume") { volume: Float ->
            synchronized(nativeLock) {
                if (handle != 0L && pipelineHandle == handle) SdaEngine.nativeSetVolume(handle, volume)
            }
        }

        OnDestroy {
            destroyed = true
            playbackCommands.shutdownNow().forEach { (it as? java.util.concurrent.Future<*>)?.cancel(true) }
            stereoReferences.clear()
            audioPolicyMonitor?.shutdownNow(); audioPolicyMonitor = null
            diagnostics?.configure(false) { JSONObject() }
            synchronized(nativeLock) {
                destroyed = true
                PlaybackKeepAlive.end(backgroundSession)
                backgroundSession = 0L
            }
            mpeghImport.close()
            stopFeedThread()
            synchronized(nativeLock) {
                if (handle != 0L) {
                    SdaEngine.nativeClose(handle)
                    handle = 0L
                }
            }
        }
    }

    private fun renderingSettings(): JSONObject {
        val context = appContext.reactContext ?: throw RuntimeException("no react context")
        val preferences = context.getSharedPreferences("sda-rendering", 0)
        if (preferences.getInt("sharedResponseVersion", 0) < 1) {
            check(preferences.edit().putInt("sharedResponseVersion", 1)
                .putBoolean("spatialEnhancementEnabled", true)
                .putBoolean("direct", true).putBoolean("directional", true).commit()) {
                "无法启用共享 HRTF 渲染"
            }
        }
        if (preferences.getInt("mobileDirectVersion", 0) < 1) {
            check(preferences.edit().putInt("mobileDirectVersion", 1).putString("hrtfSet", "dense")
                .putFloat("hrtfWetWeight", 0f).putBoolean("direct", true).putBoolean("directional", true)
                .putBoolean("nearField", false).putString("roomId", "").commit()) { "无法迁移移动端渲染设置" }
        }
        if (preferences.getInt("objectRenderingVersion", 0) < 2) {
            check(preferences.edit().putInt("objectRenderingVersion", 2).putBoolean("direct", true).putBoolean("directional", true).commit()) { "无法迁移对象渲染设置" }
        }
        return JSONObject().put("layout", activeLayout)
            // One-time migration also resets legacy object switches to the mobile defaults.
            .put("hrtfSet", "dense")
            .put("hrtfWetWeight", 0.0)
            .put("binauralEnabled", preferences.getBoolean("binauralEnabled", true))
            .put("direct", preferences.getBoolean("direct", true))
            .put("volumeBalanceEnabled", preferences.getBoolean("volumeBalanceEnabled", false))
            .put("spatialEnhancementEnabled", preferences.getBoolean("spatialEnhancementEnabled", false))
            .put("spatialCueDb", spatialCueDb())
            .put("alacStereoUpmix", preferences.getBoolean("alacStereoUpmix", false))
            .put("directional", preferences.getBoolean("directional", true))
            .put("roomId", "")
            .put("nearField", false)
            .put("metresPerUnit", preferences.getFloat("metresPerUnit", 1f).toDouble())
    }
}
