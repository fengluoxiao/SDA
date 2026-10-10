package app.sda.mobile.sda

import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test

class Media3OutputTest {
    @Test fun floatOutputBackpressurePauseFlushAndRelease() {
        val output = Media3Output(InstrumentationRegistry.getInstrumentation().context)
        try {
            assertTrue(output.open())
            assertTrue(output.setPlaying(false))
            val block = FloatArray(2048)
            var accepted = 0
            var blocked = false
            repeat(128) {
                if (!blocked) {
                    when (output.write(block)) {
                        1 -> accepted++
                        0 -> blocked = true
                        else -> fail(output.error)
                    }
                }
            }
            assertTrue("Paused output must exert backpressure", blocked)
            assertEquals(0L, output.playedFrames())
            assertTrue(output.setPlaying(true))
            val deadline = System.nanoTime() + 3_000_000_000L
            while (output.write(block) == 0 && System.nanoTime() < deadline) Thread.sleep(2)
            assertNull(output.error)
            Thread.sleep(100)
            assertTrue("Device playback clock must advance", output.playedFrames() > 0)
            assertTrue(output.flush())
            assertEquals(0L, output.playedFrames())
            assertEquals(1, output.write(block))
            assertTrue(accepted > 0)
        } finally {
            output.close()
            output.close()
        }
    }

    @Test fun captureSurvivesStartupFlushAndPreservesStereoSamples() {
        val context = InstrumentationRegistry.getInstrumentation().context
        val directory = checkNotNull(context.getExternalFilesDir(null))
        val request = java.io.File(directory, "capture-next-playback.txt")
        val result = java.io.File(directory, "media3-submitted.f32")
        result.delete()
        request.writeText("1")
        val output = Media3Output(context)
        try {
            assertTrue(output.open())
            assertTrue(output.flush()) // The real renderer flushes once at startup.
            val deadline = System.nanoTime() + 5_000_000_000L
            var frame = 0
            while (frame < 48000) {
                val count = minOf(1024, 48000 - frame)
                val block = FloatArray(count * 2) { i ->
                    val value = ((frame + i / 2) % 997) / 997f * 0.01f
                    if (i % 2 == 0) value else -value
                }
                var accepted: Int
                do {
                    accepted = output.write(block)
                    assertTrue(output.error, accepted >= 0)
                    assertTrue("Capture playback timeout", System.nanoTime() < deadline)
                    if (accepted == 0) Thread.sleep(2)
                } while (accepted == 0)
                frame += count
            }
            while ((!result.exists() || result.length() != 48000L * 8) && System.nanoTime() < deadline) Thread.sleep(10)
            assertEquals(48000L * 8, result.length())
            assertFalse(request.exists())
            val floats = java.nio.ByteBuffer.wrap(result.readBytes()).order(java.nio.ByteOrder.LITTLE_ENDIAN).asFloatBuffer()
            repeat(48000) { i ->
                val expected = (i % 997) / 997f * 0.01f
                assertEquals(expected, floats.get(), 0f)
                assertEquals(-expected, floats.get(), 0f)
            }
        } finally {
            output.close()
            request.delete()
            result.delete()
        }
    }

    @Test fun captureWindowSkipsEarlierAudioAndPreservesStereoSamples() {
        val context = InstrumentationRegistry.getInstrumentation().context
        val directory = checkNotNull(context.getExternalFilesDir(null))
        val request = java.io.File(directory, "capture-next-playback.txt")
        val result = java.io.File(directory, "media3-submitted-at-1.f32")
        result.delete()
        request.writeText("1@1")
        val output = Media3Output(context)
        try {
            assertTrue(output.open())
            assertTrue(output.flush()) // The real renderer flushes once at startup.
            val deadline = System.nanoTime() + 8_000_000_000L
            var frame = 0
            while (frame < 96000) {
                val count = minOf(1024, 96000 - frame)
                val block = FloatArray(count * 2) { i ->
                    val value = ((frame + i / 2) % 997) / 997f * 0.01f
                    if (i % 2 == 0) value else -value
                }
                var accepted: Int
                do {
                    accepted = output.write(block)
                    assertTrue(output.error, accepted >= 0)
                    assertTrue("Capture playback timeout", System.nanoTime() < deadline)
                    if (accepted == 0) Thread.sleep(2)
                } while (accepted == 0)
                frame += count
            }
            while ((!result.exists() || result.length() != 48000L * 8) && System.nanoTime() < deadline) Thread.sleep(10)
            assertEquals(48000L * 8, result.length())
            assertFalse(request.exists())
            val floats = java.nio.ByteBuffer.wrap(result.readBytes()).order(java.nio.ByteOrder.LITTLE_ENDIAN).asFloatBuffer()
            repeat(48000) { i ->
                val expected = ((48000 + i) % 997) / 997f * 0.01f
                assertEquals(expected, floats.get(), 0f)
                assertEquals(-expected, floats.get(), 0f)
            }
        } finally {
            output.close()
            request.delete()
            result.delete()
        }
    }

    /** Invoked explicitly with -e sdaReplay media3|audiotrack; same PCM for both. */
    @Test fun replayPcmForOutputComparison() {
        val backend = InstrumentationRegistry.getArguments().getString("sdaReplay")
        org.junit.Assume.assumeTrue(backend == "media3" || backend == "audiotrack")
        val context = InstrumentationRegistry.getInstrumentation().context
        val file = java.io.File(context.getExternalFilesDir(null), "output-reference.f32")
        val pcm = java.nio.ByteBuffer.wrap(file.readBytes()).order(java.nio.ByteOrder.LITTLE_ENDIAN).asFloatBuffer()
        val samples = FloatArray(pcm.remaining()).also { pcm.get(it) }
        android.util.Log.i("SdaOutputComparison", "start backend=$backend frames=${samples.size / 2}")
        val deadline = System.nanoTime() + 30_000_000_000L
        if (backend == "media3") {
            val output = Media3Output(context)
            try {
                assertTrue(output.open())
                var offset = 0
                while (offset < samples.size) {
                    val block = samples.copyOfRange(offset, minOf(offset + 2048, samples.size))
                    var accepted: Int
                    do {
                        accepted = output.write(block)
                        assertTrue(output.error, accepted >= 0)
                        assertTrue(System.nanoTime() < deadline)
                        if (accepted == 0) Thread.sleep(2)
                    } while (accepted == 0)
                    offset += block.size
                }
                while (output.playedFrames() < samples.size / 2 && System.nanoTime() < deadline) Thread.sleep(10)
                assertTrue(output.playedFrames() >= samples.size / 2)
            } finally { output.close() }
        } else {
            val track = android.media.AudioTrack.Builder()
                .setAudioAttributes(android.media.AudioAttributes.Builder()
                    .setUsage(android.media.AudioAttributes.USAGE_MEDIA)
                    .setContentType(android.media.AudioAttributes.CONTENT_TYPE_MUSIC).build())
                .setAudioFormat(android.media.AudioFormat.Builder().setSampleRate(48000)
                    .setChannelMask(android.media.AudioFormat.CHANNEL_OUT_STEREO)
                    .setEncoding(android.media.AudioFormat.ENCODING_PCM_FLOAT).build())
                .setTransferMode(android.media.AudioTrack.MODE_STREAM).setBufferSizeInBytes(32768).build()
            try {
                track.play()
                var offset = 0
                while (offset < samples.size) {
                    val count = track.write(samples, offset, minOf(2048, samples.size - offset), android.media.AudioTrack.WRITE_BLOCKING)
                    assertTrue("AudioTrack write=$count", count > 0)
                    offset += count
                    assertTrue(System.nanoTime() < deadline)
                }
                while (track.playbackHeadPosition < samples.size / 2 && System.nanoTime() < deadline) Thread.sleep(10)
                assertTrue(track.playbackHeadPosition >= samples.size / 2)
            } finally { track.release() }
        }
        android.util.Log.i("SdaOutputComparison", "complete backend=$backend")
    }
}
