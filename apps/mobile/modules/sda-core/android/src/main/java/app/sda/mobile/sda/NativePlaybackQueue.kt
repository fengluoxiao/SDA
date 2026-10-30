package app.sda.mobile.sda

/** Native owns order and EOF, including while the React runtime is suspended. */
internal data class NativeQueueTrack(val hash: String, val uri: String, val name: String,
    val metadata: String, val originalName: String = name)
internal class NativePlaybackQueue {
    private var tracks = emptyList<NativeQueueTrack>()
    private var mode = "sequence"
    @Synchronized fun configure(rows: List<NativeQueueTrack>, value: String) {
        require(value in listOf("sequence", "repeat-all", "repeat-one")) { "Invalid playback mode" }
        require(rows.map { it.hash }.distinct().size == rows.size) { "Duplicate queue identity" }
        tracks = rows.toList(); mode = value
    }
    @Synchronized fun snapshot(): List<NativeQueueTrack> = tracks.toList()
    @Synchronized fun playbackMode(): String = mode
    @Synchronized fun next(hash: String, automatic: Boolean, direction: Int = 1): NativeQueueTrack? {
        val index = tracks.indexOfFirst { it.hash == hash }
        if (index < 0 || tracks.isEmpty()) return null
        if (!automatic) return tracks[Math.floorMod(index + direction, tracks.size)]
        if (mode == "repeat-one") return tracks[index]
        return tracks.getOrNull(index + 1) ?: if (mode == "repeat-all") tracks.first() else null
    }
}
