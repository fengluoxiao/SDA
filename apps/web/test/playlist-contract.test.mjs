import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const source = readFileSync(join(import.meta.dirname, "..", "src", "App.tsx"), "utf8");
assert.match(source, /const \[playlist, setPlaylist\] = useState<PlaylistItem\[\]>\(\[\]\)/);
assert.match(source, /const appendToPlaylist = useCallback/);
assert.match(source, /path:\$\{source\.path\.toLowerCase\(\)\}/);
assert.match(source, /!playlistCurrentIdRef\.current \|\| \(!playingRef\.current && !pausedRef\.current\)/);
assert.match(source, /playlistRevisionRef\.current\+\+/);
assert.match(source, /const next = currentIndex >= 0 \? playlistRef\.current\[currentIndex \+ 1\] : null/);
assert.match(source, /floatPanel === "playlist"/);
assert.match(source, /添加文件夹/);
assert.match(source, /pickFolder/);

console.log("playlist contract tests passed");
