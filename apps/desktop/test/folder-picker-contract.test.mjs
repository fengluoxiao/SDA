import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const source = readFileSync(join(import.meta.dirname, "..", "main.cjs"), "utf8");
assert.match(source, /const MEDIA_EXTENSIONS = new Set/);
assert.match(source, /const MAX_FOLDER_MEDIA_FILES = 2000/);
assert.match(source, /function scanMediaFolder\(root\)/);
assert.match(source, /entry\.isSymbolicLink\(\)\) continue/);
assert.match(source, /entry\.isDirectory\(\)\) walk\(entryPath\)/);
assert.match(source, /entry\.isFile\(\) && isMediaFile\(entryPath\)/);
assert.match(source, /ipcMain\.handle\("sda:pick-folder"/);
assert.match(source, /properties: \["openDirectory"\]/);
assert.match(source, /media path is not a regular file/);
assert.match(source, /length > MAX_SLICE_BYTES/);

console.log("folder picker contract tests passed");
