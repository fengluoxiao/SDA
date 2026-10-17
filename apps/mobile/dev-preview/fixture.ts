import { Image } from "react-native";
import type { QueueTrack } from "../src/RemotePlayer";

// Imported only inside the __DEV__ + Expo Go gate in App.
export function createArtworkPreview() {
  const metadata = { title: "エガクミライ", artist: "MyGO!!!!!",
    coverUri: Image.resolveAssetSource(require("./egaku-cover.jpg")).uri,
    artworkColor: "#3f4e3f", durationMs: 240000 };
  const queue: QueueTrack[] = Array.from({ length: 12 }, (_, index) => ({
    contentHash: `expo-artwork-preview-${index}`, uri: `preview://artwork/${index}`,
    name: index === 0 ? "05. エガクミライ.m4a" : `界面预览 ${index + 1}.m4a`,
    metadata: index === 0 ? metadata : { ...metadata, title: `界面预览 ${index + 1}` },
  }));
  return { queue, queueIndex: 0, selectedUri: queue[0]!.uri, fileName: queue[0]!.name, metadata,
    positionMs: 0, durationMs: 240000, decodedMs: 240000, sourceCodec: "eac3",
    renderingStatus: "Dolby Atmos", paused: true, playing: false, error: null };
}
