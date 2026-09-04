import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const source = readFileSync(join(import.meta.dirname, "..", "src", "App.tsx"), "utf8");
assert.match(source, /const \[playlist, setPlaylist\] = useState<PlaylistItem\[\]>\(\[\]\)/);
assert.match(source, /const appendToPlaylist = useCallback/);
assert.match(source, /path:\$\{source\.path\.toLowerCase\(\)\}/);
assert.match(source, /!playlistCurrentIdRef\.current \|\| \(!playingRef\.current && !pausedRef\.current\)/);
assert.match(source, /const volumeRef = useRef\(volume\);\s*const volumeBalanceRef/);
assert.match(source, /volumeRef\.current = volume;\s*volumeBalanceRef\.current = volumeBalanceEnabled;/);
assert.match(source, /player\.setVolume\(volumeRef\.current\);/);
assert.match(source, /const request = \+\+playRequestRef\.current;[\s\S]*?playingRef\.current = true;\s*setPlaying\(true\);[\s\S]*?const previous = playerRef\.current;/);
assert.match(source, /playingRef\.current = false;\s*setPlaying\(false\);/);
assert.match(source, /playRequestRef\.current\+\+;\s*nativeSessionEpochRef\.current\+\+;\s*playingRef\.current = false;/);
assert.match(source, /playlistRevisionRef\.current\+\+/);
assert.match(source, /const next = currentIndex >= 0 \? playlistRef\.current\[currentIndex \+ 1\] : null/);
assert.match(source, /floatPanel === "playlist"/);
assert.match(source, /添加文件夹/);
assert.match(source, /pickFolder/);

console.log("playlist contract tests passed");
