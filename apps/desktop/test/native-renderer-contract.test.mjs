import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const main = readFileSync(join(root, "main.cjs"), "utf8");
const preload = readFileSync(join(root, "preload.cjs"), "utf8");
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));

assert.match(main, /const NATIVE_RENDERER_PROTOCOL = 1/);
assert.match(main, /function bundledNativeRendererPath\(\)/);
assert.match(main, /function startNativeRenderer\(\)/);
assert.match(main, /function stopNativeRenderer\(\)/);
assert.match(main, /native renderer.*Web Audio 回退/s);
assert.match(main, /ipcMain\.handle\("sda:native-renderer-start"/);
assert.match(main, /ipcMain\.handle\("sda:native-renderer-health"/);
assert.match(main, /stopNativeRenderer\(\);/);
assert.match(preload, /startNativeRenderer: \(\) => ipcRenderer\.invoke\("sda:native-renderer-start"\)/);
assert.match(preload, /onNativeRendererStatus/);
assert.match(pkg.scripts.dev, /build-native-renderer/);
assert.ok(pkg.build.win.extraResources.some((entry) => entry.to === "native-renderer"));

console.log("native renderer desktop bridge contract tests passed");
