package app.sda.mobile.sda

import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class RoomAssetsTest {
    private val context get() = InstrumentationRegistry.getInstrumentation().context

    @Test fun packagedDesktopRoomIsVerifiedAndUsable() {
        val rooms = RoomAssets.list(context)
        assertTrue(rooms.length() > 0)
        for (i in 0 until rooms.length()) {
            val room = rooms.getJSONObject(i)
            val path = RoomAssets.prepare(context, room.getString("id"))
            val profile = JSONObject(File(path).readText())
            assertEquals("7.1.4", profile.getString("layout"))
            assertEquals(48000, profile.getInt("sampleRate"))
            assertEquals("simulated", profile.getString("measurement"))
            assertTrue(profile.getJSONArray("speakers").length() > 0)
            assertEquals(path, RoomAssets.prepare(context, room.getString("id")))
            // A corrupt extracted cache must be replaced from the verified archive.
            File(path).writeText("invalid")
            assertEquals(path, RoomAssets.prepare(context, room.getString("id")))
            assertEquals(profile.getString("name"), JSONObject(File(path).readText()).getString("name"))
        }
    }

    @Test fun bypassNeedsNoRoomFile() { assertEquals("", RoomAssets.prepare(context, "")) }

    @Test fun unknownRoomCannotEscapeAssetDirectory() {
        try {
            RoomAssets.prepare(context, "../../outside")
            fail("unknown room accepted")
        } catch (_: IllegalStateException) { }
    }
}
