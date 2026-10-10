package com.sda.engine

import android.app.Activity
import android.os.Bundle
import android.util.Log
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import org.json.JSONObject
import java.io.File

class MainActivity : Activity() {
    private val engine = com.sda.nativebridge.SdaEngine

    @Volatile private var stopRequested = false
    private var worker: Thread? = null
    private lateinit var status: TextView
    private lateinit var play: Button
    private lateinit var stop: Button

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val layout = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(48, 80, 48, 48)
        }
        layout.addView(TextView(this).apply {
            text = "SDA · 真歌试听"
            textSize = 24f
        })
        status = TextView(this).apply {
            textSize = 18f
            text = "已就绪，点击播放。默认 25% 音量。\n播放时可随时停止；离开页面也会停止。"
            setPadding(0, 24, 0, 24)
        }
        play = Button(this).apply {
            text = "播放歌曲（低音量）"
            setOnClickListener { startPlayback(false) }
        }
        stop = Button(this).apply {
            text = "停止"
            isEnabled = false
            setOnClickListener {
                stopRequested = true
                isEnabled = false
                status.text = "正在停止…"
            }
        }
        layout.addView(status)
        layout.addView(play)
        layout.addView(stop)
        setContentView(layout)

        // Automated validation explicitly requests silence; normal launches never autoplay.
        if (intent.getBooleanExtra("validationMuted", false)) {
            startPlayback(true)
        }
    }

    private fun startPlayback(muted: Boolean) {
        if (worker != null) return
        stopRequested = false
        play.isEnabled = false
        stop.isEnabled = true
        status.text = "正在准备音频…"
        worker = Thread({
            var ptr = 0L
            var resultText = "已停止，可重新播放。"
            try {
                val hrtfDir = File(filesDir, "hrtf").apply { mkdirs() }
                for (name in assets.list("hrtf") ?: emptyArray()) {
                    assets.open("hrtf/$name").use { input ->
                        File(hrtfDir, name).outputStream().use { output -> input.copyTo(output) }
                    }
                }
                ptr = engine.nativeInit(
                    """{"sampleRate":48000,"outputChannels":2,"layout":"7.1.4"}""",
                    File(hrtfDir, "hrtf-set.json").absolutePath
                )
                check(ptr != 0L) { "引擎初始化失败" }
                check(nativeStart(ptr) == 0) { "音频输出启动失败" }
                check(nativeSetVolume(ptr, if (muted) 0f else 0.25f) == 0) { "音量设置失败" }
                val maxLead = 2L * 48000L
                var lastUpdate = 0L
                var lastProgress = System.nanoTime()
                var lastConsumed = 0L
                assets.open("song.eac3").use { input ->
                    val buffer = ByteArray(4096)
                    while (!stopRequested) {
                        val state = JSONObject(nativeStatus(ptr))
                        // Missing status fields are an incompatible native library, not zero progress.
                        val decoded = state.getLong("decodedSamplePos")
                        val consumed = state.getLong("consumedSamplePos")
                        val now = System.nanoTime()
                        if (consumed != lastConsumed) {
                            lastProgress = now
                            lastConsumed = consumed
                        }
                        check(decoded == 0L || now - lastProgress < 15_000_000_000L) {
                            "播放进度停止，请查看音频日志"
                        }
                        if (now - lastUpdate > 1_000_000_000L) {
                            Log.i("SdaEngine", "playback_status=$state muted=$muted")
                            val seconds = consumed / 48000L
                            runOnUiThread {
                                status.text = "${if (muted) "静音验证" else "正在播放"}：${seconds / 60}:${(seconds % 60).toString().padStart(2, '0')} / 3:31\n音频输出：PCM_FLOAT，48 kHz 双声道"
                            }
                            lastUpdate = now
                        }
                        if (decoded - consumed > maxLead) {
                            Thread.sleep(20)
                            continue
                        }
                        val count = input.read(buffer)
                        if (count < 0) break
                        check(nativeFeed(ptr, if (count == buffer.size) buffer else buffer.copyOf(count)) >= 0) {
                            "音频数据提交失败"
                        }
                    }
                }
                if (!stopRequested) {
                    check(nativeFinish(ptr) >= 0) { "音频尾帧提交失败" }
                    val deadline = System.nanoTime() + 15_000_000_000L
                    while (!stopRequested) {
                        val state = JSONObject(nativeStatus(ptr))
                        if (state.getLong("consumedSamplePos") >= state.getLong("decodedSamplePos")) {
                            Log.i("SdaEngine", "playback_complete=$state muted=$muted")
                            resultText = "播放完成，可重新播放。"
                            break
                        }
                        check(System.nanoTime() < deadline) { "等待播放结束超时" }
                        Thread.sleep(20)
                    }
                }
            } catch (error: Exception) {
                Log.e("SdaEngine", "playback failed", error)
                resultText = "播放失败：${error.message}"
            } finally {
                // All JNI handle access stays on this thread, including close.
                if (ptr != 0L) engine.nativeClose(ptr)
                runOnUiThread {
                    status.text = resultText
                    worker = null
                    play.isEnabled = true
                    stop.isEnabled = false
                }
            }
        }, "sda-song-feed").also { it.start() }
    }

    override fun onStop() {
        stopRequested = true
        super.onStop()
    }
}
