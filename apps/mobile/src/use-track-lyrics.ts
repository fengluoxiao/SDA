import * as DocumentPicker from "expo-document-picker";
import React from "react";
import { readExternalLyric, releaseLyricPickerCopy, readFileLyrics } from "./lyric-files";
import { lyricSources, parseLyrics, type LyricDocument, type LyricSource } from "./mobile-lyrics";

export function useTrackLyrics({ uri, metadataLyrics, active, attach }: {
  uri: string; metadataLyrics: unknown; active: boolean; attach?(uri: string, source: LyricSource): void;
}) {
  const [document, setDocument] = React.useState<LyricDocument | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [importing, setImporting] = React.useState(false);
  const [error, setError] = React.useState("");
  const generation = React.useRef(0), currentUri = React.useRef(uri), loadedKey = React.useRef<{ uri: string; lyrics: unknown; completed: boolean } | undefined>(undefined); currentUri.current = uri;
  React.useEffect(() => {
    const token = ++generation.current;
    if (loadedKey.current?.uri !== uri || loadedKey.current?.lyrics !== metadataLyrics) { setDocument(null); setError(""); loadedKey.current = { uri, lyrics: metadataLyrics, completed: false }; }
    setLoading(false);
    if (!active || !uri || loadedKey.current?.completed) return;
    let cancelled = false;
    const publish = (doc: LyricDocument | null) => { if (!cancelled && token === generation.current) { if (loadedKey.current) loadedKey.current.completed = true; setDocument(doc); } };
    const choose = (sources: LyricSource[]) => {
      let fallback: LyricDocument | null = null;
      for (const source of sources) {
        try { const doc = parseLyrics(source); if (doc.lines.length) { if (doc.timed) return doc; fallback ??= doc; } } catch { /* Optional embedded tags must not interrupt audio. */ }
      }
      return fallback;
    };
    setLoading(true);
    void (async () => {
      const supplied = choose(lyricSources(metadataLyrics));
      if (supplied?.source.source === "external" || supplied?.timed || !uri.startsWith("file:")) { publish(supplied); return; }
      const sources = await readFileLyrics(uri, () => cancelled || token !== generation.current);
      publish(choose(sources) ?? supplied);
    })().catch(() => { publish(choose(lyricSources(metadataLyrics))); }).finally(() => { if (!cancelled && token === generation.current) setLoading(false); });
    return () => { cancelled = true; };
  }, [uri, metadataLyrics, active]);
  const importLyrics = React.useCallback(async () => {
    if (!uri || importing) return;
    const sourceUri = uri; setImporting(true); setError("");
    let pickerUri = "";
    try {
      const result = await DocumentPicker.getDocumentAsync({ type: "*/*", copyToCacheDirectory: true, multiple: false });
      if (result.canceled || !result.assets[0]) return;
      const file = result.assets[0]; pickerUri = file.uri;
      const source = await readExternalLyric(file.uri, file.name, file.size);
      if (currentUri.current !== sourceUri) return;
      attach?.(sourceUri, source); setDocument(parseLyrics(source));
    } catch (e) { if (currentUri.current === sourceUri) setError(e instanceof Error ? e.message : String(e)); }
    finally { if (pickerUri) await releaseLyricPickerCopy(pickerUri); setImporting(false); }
  }, [uri, importing, attach]);
  return { document, loading, importing, error, importLyrics };
}
