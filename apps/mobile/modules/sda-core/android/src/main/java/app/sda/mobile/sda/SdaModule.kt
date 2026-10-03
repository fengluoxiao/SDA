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
    private var handle: Long = 0L
    private var activeLayout = "7.1.4"
    @Volatile private var activeIsMp3 = false
    private var feedThread: Thread? = null
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

    private fun ensureEngine(layout: String = "7.1.4"): Long = synchronized(nativeLock) {
        if (handle != 0L) return@synchronized handle
        val context = appContext?.reactContext ?: throw RuntimeException("no react context")
        // Ask Android to provision the app-owned directory (also used by
        // opt-in native PCM diagnostic builds), rather than creating it via adb.
        context.getExternalFilesDir(null)
        // One shared direct-only HRIR grid serves objects and bed channels.
        for (assetDirectory in listOf("hrtf-mobile-direct")) {
            val hrtfDir = java.io.File(context.filesDir, assetDirectory)
            check(hrtfDir.mkdirs() || hrtfDir.isDirectory) { "Cannot create KU100 asset directory: $hrtfDir" }
            val names = context.assets.list(assetDirectory)?.toList().orEmpty()
            val manifestName = "hrtf-set.json"
            check(manifestName in names && names.count { it.endsWith("-dry.f32") } == 128 && "zero-wet.f32" in names) {
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
        hrtfLoadStatus = "KU100 资源已就绪，正在加载"
        val settings = renderingSettings()
        val config = JSONObject().put("sampleRate", 48000).put("outputChannels", 2)
            .put("layout", layout).put("directObjectHrtf", settings.getBoolean("direct"))
            .put("directionalHrtf", settings.getBoolean("directional"))
            .put("hrtfWetWeight", settings.getDouble("hrtfWetWeight")).toString()
        val assetDirectory = "hrtf-mobile-direct"
        val manifest = java.io.File(context.filesDir, "$assetDirectory/hrtf-set.json")
        val directionCount = JSONObject(manifest.readText()).getJSONArray("positions").length()
        val ptr = SdaEngine.nativeInit(config, manifest.absolutePath)
        if (ptr == 0L) {
            val detail = SdaEngine.nativeInitError()
            hrtfLoadStatus = "KU100 加载失败: ${if (detail.isBlank()) "nativeInit failed" else detail}"
            throw RuntimeException(hrtfLoadStatus)
        }
        handle = ptr
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
        hrtfLoadStatus = "已加载 KU100 D1 · $directionCount 方向 · $flavor · $wet (Apache-2.0)"
        val result = SdaEngine.nativeStart(ptr)
        if (result != 0) {
            val detail = SdaEngine.nativeLastError().ifBlank { "unknown native output error" }
            SdaEngine.nativeClose(ptr)
            handle = 0L
            throw RuntimeException("nativeStart failed: $detail ($result)")
        }
        ptr
    }

    private fun stopFeedThread() {
        synchronized(lifecycleLock) {
            stopped = true
            try {
                activeInput?.close()
            } catch (_: Throwable) {
            }
            activeInput = null
            val worker = feedThread
            worker?.interrupt()
            if (worker != null) {
                worker.join(2_000)
                check(!worker.isAlive) { "Timed out stopping audio feed thread; native handle retained" }
            }
            feedThread = null
        }
    }

    override fun definition() = ModuleDefinition {
        Name("SdaEngine")

        AsyncFunction("contentHash") { uriString: String ->
            val context = appContext.reactContext ?: throw RuntimeException("no react context")
            MediaMetadata.contentHash(context, Uri.parse(uriString))
        }

        AsyncFunction("metadata") { uriString: String ->
            val context = appContext.reactContext ?: throw RuntimeException("no react context")
            MediaMetadata.read(context, Uri.parse(uriString)).toString()
        }

        AsyncFunction("durationMs") { uriString: String ->
            val context = appContext.reactContext ?: throw RuntimeException("no react context")
            Eac3Input.durationMs(context, Uri.parse(uriString))
        }

        Function("setVolumeBalance") { enabled: Boolean ->
            synchronized(nativeLock) {
                if (handle != 0L) {
                    val error = SdaEngine.nativeSetVolumeBalance(handle, enabled)
                    check(error.isEmpty()) { error }
                }
                val context = appContext.reactContext ?: error("no react context")
                context.getSharedPreferences("sda-rendering", 0).edit()
                    .putBoolean("volumeBalanceEnabled", enabled).apply()
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
                require(!profile.getBoolean("nearField") && profile.getString("roomId").isEmpty()) { "不支持的预设房间配置" }
                if (handle != 0L) {
                    val directory = "hrtf-mobile-direct"
                    val manifest = java.io.File(context.filesDir, "$directory/hrtf-set.json")
                    val error = SdaEngine.nativeSetHrtfPreset(handle, manifest.absolutePath,
                        wetWeight.toFloat(), profile.getBoolean("direct"), profile.getBoolean("directional"))
                    check(error.isEmpty()) { error }
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
                if (handle != 0L) {
                    val error = SdaEngine.nativeSetRoom(handle, path)
                    check(error.isEmpty()) { error }
                }
                val preferences = context.getSharedPreferences("sda-rendering", 0).edit().putString("roomId", id)
                preferences.apply()
            }
        }

        Function("setObjectRendering") { direct: Boolean, directional: Boolean ->
            synchronized(nativeLock) {
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
            require(headYawDegrees.isFinite() && headYawDegrees in -180.0..180.0) { "Head yaw must be finite and between -180 and 180 degrees" }
            require(contentHash.matches(Regex("[0-9a-f]{64}"))) { "Invalid track content hash" }
            stopFeedThread()
            val context = appContext?.reactContext ?: throw RuntimeException("no react context")
            val uri = Uri.parse(uriString)
            synchronized(nativeLock) {
                if (handle != 0L) {
                    SdaEngine.nativeFinish(handle)
                    SdaEngine.nativeClose(handle)
                    handle = 0L
                }
            }
            // Versioned full-content identity survives temporary MP4 -> MHAS
            // imports and cannot reuse a result for a changed source file.
            val cacheKey = "sda-measured-lufs-v6:$contentHash:48000"
            val loudnessCache = context.getSharedPreferences("sda-loudness", 0)
            val isMpegh = displayName.substringAfterLast('.', "").equals("mhas", ignoreCase = true)
            val isMp3 = displayName.substringAfterLast('.', "").equals("mp3", ignoreCase = true)
            val mp3Cache = if (isMp3) java.io.File.createTempFile("sda-mp3-", ".mp3", context.cacheDir) else null
            if (mp3Cache != null) {
                try {
                    val source = context.contentResolver.openInputStream(uri) ?: error("无法打开所选 MP3 文件")
                    source.use { stream -> mp3Cache.outputStream().use { stream.copyTo(it, 64 * 1024) } }
                } catch (error: Throwable) {
                    mp3Cache.delete()
                    throw error
                }
            }
            val input = if (isMp3) java.io.ByteArrayInputStream(ByteArray(0)) else Eac3Input.open(context, uri, displayName)
            val ptr = try {
                ensureEngine(if (isMpegh) "360RA-13" else "7.1.4").also {
                    if (isMpegh) check(SdaEngine.nativeOpenMpegh(it) == 0) {
                        "360RA 打开失败: ${SdaEngine.nativeLastError()}"
                    }
                    if (mp3Cache != null) check(SdaEngine.nativeOpenMp3(it, mp3Cache.absolutePath) > 0) {
                        "MP3 打开失败: ${SdaEngine.nativeLastError()}"
                    }
                    val balanceError = SdaEngine.nativeSetVolumeBalance(it, renderingSettings().getBoolean("volumeBalanceEnabled"))
                    check(balanceError.isEmpty()) { balanceError }
                    loudnessCache.getString(cacheKey, null)?.let { cached ->
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
                }
            } catch (error: Throwable) {
                input.close()
                mp3Cache?.delete()
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
                generation
            }
            feedError = null
            feedDone = false
            paused = false
            stopped = false
            activeIsMp3 = isMp3
            activeInput = input
            val worker = Thread({
                try {
                    input.use { stream ->
                        val buffer = ByteArray(24 * 1024)
                        var lastConsumed = 0L
                        var lastProgressNs = System.nanoTime()
                        // player.ts TARGET_AHEAD_SECONDS: same four-second decode
                        // reserve, paced against presented audio rather than FIFO size.
                        val maxLead = 4 * 48_000L
                        while (!stopped) {
                            val state = synchronized(nativeLock) {
                                if (generation != workerGeneration || handle != ptr || stopped) return@Thread
                                JSONObject(SdaEngine.nativeStatus(ptr))
                            }
                            val decoded = state.getLong("decodedSamplePos")
                            val consumed = state.getLong("consumedSamplePos")
                            val fifoFrames = state.optInt("fifoFrames", 0)
                            if (paused) {
                                lastProgressNs = System.nanoTime()
                                Thread.sleep(20)
                                continue
                            }
                            if (consumed != lastConsumed) {
                                lastConsumed = consumed
                                lastProgressNs = System.nanoTime()
                            }
                            check(decoded == 0L || System.nanoTime() - lastProgressNs < 15_000_000_000L) {
                                "Audio consumption stalled (decoded=$decoded consumed=$consumed)"
                            }
                            if (decoded - consumed > maxLead || fifoFrames > maxLead) {
                                Thread.sleep(20)
                                continue
                            }
                            if (isMp3) {
                                val pulled = synchronized(nativeLock) {
                                    if (generation != workerGeneration || handle != ptr || stopped) return@Thread
                                    SdaEngine.nativePullMp3(ptr, 4096)
                                }
                                if (pulled == -4) break
                                check(pulled >= 0) { "MP3 解码失败: ${SdaEngine.nativeLastError()}" }
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
                            check(result >= 0) { "nativeFeed failed: ${SdaEngine.nativeLastError()}" }
                        }
                        if (!stopped) {
                            var eofDeadline = System.nanoTime() + 15_000_000_000L
                            synchronized(nativeLock) {
                                if (generation == workerGeneration && handle == ptr && !stopped) {
                                    val result = SdaEngine.nativeFinish(ptr)
                                    check(result >= 0) { "nativeFinish failed: ${SdaEngine.nativeLastError()}" }
                                    // Match Windows: save only successfully flushed full
                                    // decodes, not a stopped intro or a failed feed.
                                    val measurement = SdaEngine.nativeCompleteLoudness(ptr)
                                    if (measurement != "null") loudnessCache.edit().putString(cacheKey, measurement).apply()
                                }
                            }
                            while (!stopped) {
                                val state = synchronized(nativeLock) {
                                    if (generation != workerGeneration || handle != ptr || stopped) return@Thread
                                    JSONObject(SdaEngine.nativeStatus(ptr))
                                }
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
                                }
                            }
                        }
                    }
                } catch (error: InterruptedException) {
                    Thread.currentThread().interrupt()
                } catch (error: Throwable) {
                    feedError = error.message ?: error.toString()
                    synchronized(nativeLock) {
                        if (generation == workerGeneration && handle == ptr) {
                            SdaEngine.nativeClose(ptr)
                            handle = 0L
                        }
                    }
                    feedDone = true
                } finally {
                    mp3Cache?.delete()
                }
            }, "sda-content-feed")
            feedThread = worker
            worker.start()
            displayName
        }

        Function("pause") { ->
            synchronized(nativeLock) {
                if (handle == 0L) return@synchronized false
                check(SdaEngine.nativePause(handle, true) == 0) { "native pause failed" }
                paused = true
                true
            }
        }

        Function("resume") { ->
            synchronized(nativeLock) {
                if (handle == 0L) return@synchronized false
                check(SdaEngine.nativePause(handle, false) == 0) { "native resume failed" }
                paused = false
                true
            }
        }

        Function("stop") { ->
            stopFeedThread()
            synchronized(nativeLock) {
                if (handle != 0L) {
                    SdaEngine.nativeFinish(handle)
                    SdaEngine.nativeClose(handle)
                    handle = 0L
                }
            }
            feedDone = true
            true
        }

        Function("status") { ->
            synchronized(nativeLock) {
                if (handle == 0L) "{}" else SdaEngine.nativeStatus(handle)
            }
        }

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
            }
        }

        Function("resetHeadPose") { ->
            synchronized(nativeLock) {
                check(handle != 0L && SdaEngine.nativeResetHeadPose(handle) == 0) { "Native head pose reset failed" }
            }
        }

        Function("hrtfStatus") { -> hrtfLoadStatus }

        Function("feedError") { -> feedError }

        Function("feedDone") { -> feedDone }

        Function("stereoBedMode") { -> activeIsMp3 }
        Function("nativeLastError") { -> SdaEngine.nativeLastError() }

        Function("setVolume") { volume: Float ->
            synchronized(nativeLock) {
                if (handle != 0L) SdaEngine.nativeSetVolume(handle, volume)
            }
        }

        OnDestroy {
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
        if (preferences.getInt("mobileDirectVersion", 0) < 1) {
            check(preferences.edit().putInt("mobileDirectVersion", 1).putString("hrtfSet", "dense")
                .putFloat("hrtfWetWeight", 0f).putBoolean("direct", true).putBoolean("directional", true)
                .putBoolean("nearField", false).putString("roomId", "").commit()) { "无法迁移移动端渲染设置" }
        }
        return JSONObject().put("layout", activeLayout)
            // One-time migration also resets legacy object switches to the mobile defaults.
            .put("hrtfSet", "dense")
            .put("hrtfWetWeight", 0.0)
            .put("direct", preferences.getBoolean("direct", true))
            .put("volumeBalanceEnabled", preferences.getBoolean("volumeBalanceEnabled", false))
            .put("directional", preferences.getBoolean("directional", true))
            .put("roomId", "")
            .put("nearField", false)
            .put("metresPerUnit", preferences.getFloat("metresPerUnit", 1f).toDouble())
    }
}
