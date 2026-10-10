package app.sda.mobile.sda

import android.content.Context
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import androidx.annotation.Keep
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.exoplayer.audio.DefaultAudioSink
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.FutureTask

/** Consumes native KU100 output only. Media3 never decodes/downmixes the Atmos input. */
@Keep
@androidx.media3.common.util.UnstableApi
class Media3Output(private val context: Context) {
    @Volatile var error: String? = null
        private set
    private val thread = HandlerThread("sda-media3").apply { start() }
    private val handler = Handler(thread.looper)
    private var sink: DefaultAudioSink? = null
    private val buffer = ByteBuffer.allocateDirect(1024 * 2 * 4).order(ByteOrder.nativeOrder())
    private var capture: SubmittedPcmCapture? = null
    private var pending = false
    private var frames = 0L
    private var lastReport = 0L
    @Volatile private var closed = false

    // All Media3 operations run on one live Looper, independently of the engine lock.
    private fun <T> onOutput(action: () -> T): T {
        val task = FutureTask<T> { action() }
        check(handler.post(task)) { "Media3 output thread stopped" }
        return task.get()
    }

    private fun operation(action: () -> Unit): Boolean = try {
        onOutput(action)
        true
    } catch (failure: Exception) {
        error = "Media3: ${failure.cause?.message ?: failure.message}"
        Log.e("SdaMedia3", error, failure)
        false
    }

    fun open(): Boolean = operation {
        capture = SubmittedPcmCapture.arm(context)
        sink = DefaultAudioSink.Builder(context).setEnableFloatOutput(true).build().also {
            // Only this app's stream is affected; never change global OEM sound settings.
            it.setAudioAttributes(AudioAttributes.Builder()
                .setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC)
                .setSpatializationBehavior(C.SPATIALIZATION_BEHAVIOR_NEVER)
                .build())
            it.configure(Format.Builder().setSampleMimeType(MimeTypes.AUDIO_RAW)
                .setSampleRate(48000).setChannelCount(2).setPcmEncoding(C.ENCODING_PCM_FLOAT)
                .build(), 32768, null)
            it.play()
        }
        Log.i("SdaMedia3", "output=Media3 DefaultAudioSink PCM_FLOAT stereo 48000Hz; native KU100 PCM; requested spatialization=NEVER")
    }

    /** 1 = buffer accepted, 0 = retry same buffer, -1 = terminal output error. */
    fun write(samples: FloatArray): Int = try {
        onOutput {
            if (!pending) {
                buffer.clear()
                buffer.asFloatBuffer().put(samples)
                buffer.limit(samples.size * 4)
                pending = true
            }
            if (checkNotNull(sink).handleBuffer(buffer, frames * 1_000_000L / 48000, 1)) {
                capture?.accepted(samples)
                frames += samples.size / 2
                pending = false
                if (frames - lastReport >= 240000) {
                    lastReport = frames
                    Log.i("SdaMedia3", "accepted_frames=$frames position_us=${sink?.getCurrentPositionUs(false)}")
                }
                1
            } else 0
        }
    } catch (failure: Exception) {
        error = "Media3: ${failure.cause?.message ?: failure.message}"
        Log.e("SdaMedia3", error, failure)
        -1
    }

    fun flush(): Boolean = operation {
        // Startup flushes precede the first PCM; only a later seek invalidates capture.
        if (frames > 0 && capture != null) {
            Log.w("SdaMedia3", "PCM capture invalidated by flush after submitted_frames=$frames")
            capture = null
        }
        Log.i("SdaMedia3", "flush submitted_frames=$frames")
        sink?.flush()
        pending = false
        frames = 0
        lastReport = 0
    }

    fun playedFrames(): Long = try {
        onOutput {
            val position = sink?.getCurrentPositionUs(false) ?: 0L
            // Media3 truncates frames to microseconds; round back up to avoid
            // leaving a fractional microsecond (one frame) stuck at EOF.
            if (position < 0) 0L else ((position * 48000L + 999999L) / 1_000_000L).coerceAtMost(frames)
        }
    } catch (failure: Exception) {
        error = "Media3 playback clock: ${failure.cause?.message ?: failure.message}"
        -1L
    }

    fun setPlaying(playing: Boolean): Boolean = operation {
        if (playing) sink?.play() else sink?.pause()
    }

    @Synchronized fun close() {
        if (closed) return
        closed = true
        operation { sink?.reset(); sink = null; pending = false; capture = null }
        thread.quitSafely()
    }
}

/** One-shot, opt-in diagnostic: no disk IO during PCM submission. */
private class SubmittedPcmCapture(private val directory: java.io.File, seconds: Int,
    private val startSeconds: Int = 0) {
    private var submittedSamples = 0L
    private val startSample = startSeconds * 48000L * 2
    private var data: ByteBuffer? = ByteBuffer.allocate(seconds * 48000 * 2 * 4)
        .order(ByteOrder.LITTLE_ENDIAN)

    fun accepted(samples: FloatArray) {
        val target = data ?: return
        val offset = (startSample - submittedSamples).coerceIn(0L, samples.size.toLong()).toInt()
        submittedSamples += samples.size
        val count = minOf(samples.size - offset, target.remaining() / 4)
        if (count == 0) return
        target.asFloatBuffer().put(samples, offset, count)
        target.position(target.position() + count * 4)
        if (!target.hasRemaining()) {
            data = null
            Thread({
                try {
                    val name = if (startSeconds == 0) "media3-submitted.f32" else "media3-submitted-at-$startSeconds.f32"
                    java.io.File(directory, name).writeBytes(target.array())
                    Log.i("SdaMedia3", "PCM capture complete start=$startSeconds frames=${target.capacity() / 8}")
                } catch (failure: Exception) {
                    Log.w("SdaMedia3", "PCM capture save failed", failure)
                }
            }, "sda-pcm-capture").start()
        }
    }

    companion object {
        fun arm(context: Context): SubmittedPcmCapture? = try {
            val directory = context.getExternalFilesDir(null)
            val request = directory?.let { java.io.File(it, "capture-next-playback.txt") }
            if (request == null || !request.isFile) null else {
                val parts = request.readText().trim().split("@")
                require(parts.size in 1..2)
                val seconds = parts[0].toInt()
                val startSeconds = parts.getOrNull(1)?.toInt() ?: 0
                require(seconds in 1..60 && startSeconds in 0..600)
                check(request.delete())
                java.io.File(directory, "media3-submitted-settings.json").writeText(
                    org.json.JSONObject(context.getSharedPreferences("sda-rendering", 0).all)
                        .put("sampleRate", 48000).put("channels", 2)
                        .put("encoding", "float32-le").put("seconds", seconds)
                        .put("startSeconds", startSeconds).toString())
                Log.i("SdaMedia3", "PCM capture armed seconds=$seconds start=$startSeconds")
                SubmittedPcmCapture(directory, seconds, startSeconds)
            }
        } catch (failure: Exception) {
            Log.w("SdaMedia3", "PCM capture unavailable", failure)
            null
        }
    }
}
