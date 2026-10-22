package app.sda.mobile.sda

import android.net.Uri
import android.util.Base64
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.Assert.*
import org.junit.Test

class PlaybackPreparationTest {
    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext
    @Test fun completedPreparationReplacesOnlyOwnedPickerCopy() {
        val directory=File(context.cacheDir,"DocumentPicker").apply { mkdirs() }
        val original=File.createTempFile("sda-import-test-",".m4a",directory).apply { writeBytes(byteArrayOf(1,2,3,4)) }
        val bridge=MpeghImport()
        val token=org.json.JSONObject(bridge.begin(context,Uri.fromFile(original))).getString("token")
        try {
            assertTrue(original.isFile)
            bridge.append(token,Base64.encodeToString(byteArrayOf(9,8,7),Base64.NO_WRAP))
            val result=File(Uri.parse(bridge.finish(token)).path!!)
            assertTrue(original.isFile)
            assertArrayEquals(byteArrayOf(9,8,7),result.readBytes())
            bridge.adopt(context,token)
            assertFalse(original.exists())
            assertTrue(result.isFile)
            bridge.discard(token)
            assertFalse(result.exists())
        } finally { bridge.close();original.delete() }
    }
    @Test fun directSourceAndUnfinishedImportAreNeverConsumed() {
        val original=File.createTempFile("sda-direct-source-",".m4a",context.cacheDir).apply { writeText("source") }
        val bridge=MpeghImport()
        val token=org.json.JSONObject(bridge.begin(context,Uri.fromFile(original))).getString("token")
        try {
            try {bridge.adopt(context,token);fail("unfinished import accepted")}catch(expected:IllegalStateException){}
            assertTrue(original.isFile)
            bridge.append(token,Base64.encodeToString(byteArrayOf(1),Base64.NO_WRAP))
            bridge.finish(token);bridge.adopt(context,token);bridge.discard(token)
            assertEquals("source",original.readText())
            assertFalse(MpeghImport.discardPickerCopy(context,Uri.fromFile(original)))
        } finally {bridge.close();original.delete()}
    }
    @Test fun repeatedTransientLossRetainsResumeIntent() {
        val state=AudioInterruptionState();state.granted();state.lost(true,true)
        state.lost(true,false)
        assertTrue(state.resumeOnGain);assertTrue(state.gained())
        state.resumed();assertFalse(state.resumeOnGain)
    }
    @Test fun permanentLossCannotAutoResumeFromLateGain() {
        val state=AudioInterruptionState();state.granted();state.lost(true,true);state.lost(false,false)
        assertFalse(state.gained());assertFalse(state.held);assertTrue(state.pauseRequested)
    }
}
