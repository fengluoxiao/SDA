import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { runInNewContext } from "node:vm";

const root = join(import.meta.dirname, "..");
const main = readFileSync(join(root, "main.cjs"), "utf8");
const preload = readFileSync(join(root, "preload.cjs"), "utf8");
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));

assert.match(main, /const NATIVE_RENDERER_PROTOCOL = 6/);
assert.match(main, /function bundledNativeRendererPath\(\)/);
const rendererPathFunction = main.match(/function bundledNativeRendererPath\(\) \{[\s\S]*?\r?\n\}/)?.[0];
assert.ok(rendererPathFunction);
for (const isPackaged of [false, true]) {
  for (const isDev of [false, true]) {
    const resourcesPath = join(root, "electron-resources");
    const expected = join(isPackaged ? resourcesPath : root, "native-renderer", "SdaNativeRenderer.exe");
    for (const exists of [false, true]) {
      const actual = runInNewContext(`${rendererPathFunction}\nbundledNativeRendererPath()`, {
        app: { isPackaged },
        isDev,
        process: { resourcesPath },
        __dirname: root,
        path: { join },
        fs: { existsSync: (candidate) => exists && candidate === expected },
      });
      assert.equal(actual, exists ? expected : null, `isPackaged=${isPackaged}, isDev=${isDev}, exists=${exists}`);
    }
  }
}
assert.match(main, /function startNativeRenderer\(\)/);
assert.match(main, /function stopNativeRenderer\(\)/);
assert.match(main, /native object renderer owns desktop audible output/i);
assert.match(main, /ipcMain\.handle\("sda:native-renderer-start"/);
assert.match(main, /ipcMain\.handle\("sda:native-renderer-health"/);
assert.match(main, /function nativeRendererBatch\(start, entries\)/);
assert.match(main, /sda:native-renderer-remove-source/);
assert.match(main, /sda:native-renderer-source/);
assert.match(main, /bedLabel/);
assert.match(main, /sda:native-renderer-lfe-muted/);
assert.match(main, /setLfeMuted/);
assert.match(main, /sda:native-renderer-volume/);
assert.match(main, /sda:native-renderer-program-enabled/);
assert.match(main, /sda:native-renderer-program-gain/);
assert.match(main, /sda:native-renderer-binaural-eq/);
assert.match(main, /sda:native-renderer-headphone-profile/);
assert.match(main, /nativeRendererHeadphoneFir/);
assert.match(main, /nativeRendererControlChain/);
assert.match(main, /sda:native-renderer-events/);
assert.match(main, /sda:native-renderer-pose/);
assert.match(main, /sda:native-renderer-hrtf/);
assert.match(main, /sda:native-renderer-layout/);
assert.match(main, /type: "setLayout", layout/);
assert.match(main, /nativeRendererCommandAck\(\{ type: "setLayout", layout \}, "setLayout"\)/);
assert.match(main, /SDA_HRTF_ROOT/);
assert.match(main, /sda:native-renderer-output-active/);
assert.match(main, /sda:native-renderer-start-at/);
assert.match(main, /sda:native-renderer-pause/);
assert.match(main, /nativeRendererPendingBatches/);
assert.match(main, /message\?\.type === "ack"/);
assert.match(main, /function nativeRendererCommandAck\(command, ackCommand\)/);
assert.match(main, /message\?\.type === "batchAck"/);
assert.match(main, /message\?\.type === "objectActivity"/);
assert.match(main, /function publishNativeRendererObjectActivity\(ids\)/);
assert.match(main, /sda:native-renderer-object-activity/);
assert.match(main, /publishNativeRendererObjectActivity\(\[\]\)/);
assert.match(main, /nativeRendererHealthTimer = setInterval/);
assert.match(main, /function clearNativeRendererSession\(reason\)/);
assert.match(main, /nativeRenderer\.stdin\.on\("error"/);
assert.match(main, /outputActive: message\.outputActive === true/);
assert.match(main, /samplePos: Number\(message\.samplePos\)/);
assert.match(preload, /nativeRendererRemoveSource/);
assert.match(preload, /nativeRendererSource: \(source\)/);
assert.match(preload, /nativeRendererLfeMuted/);
assert.match(preload, /nativeRendererVolume/);
assert.match(preload, /nativeRendererProgramEnabled/);
assert.match(preload, /nativeRendererProgramGain/);
assert.match(preload, /nativeRendererBinauralEq/);
assert.match(preload, /nativeRendererHeadphoneProfile/);
assert.match(preload, /nativeRendererEvents/);
assert.match(preload, /nativeRendererPose/);
assert.match(preload, /nativeRendererLayout/);
assert.match(main, /stopNativeRenderer\(\);/);
assert.match(preload, /startNativeRenderer: \(\) => ipcRenderer\.invoke\("sda:native-renderer-start"\)/);
assert.match(preload, /onNativeRendererStatus/);
assert.match(preload, /onNativeRendererObjectActivity/);
assert.match(pkg.scripts.dev, /build-native-renderer/);
assert.ok(pkg.build.win.extraResources.some((entry) => entry.to === "native-renderer"));

console.log("native renderer desktop bridge contract tests passed");

const directHandler = main.match(/ipcMain\.handle\("sda:native-renderer-object-hrtf",[\s\S]*?\r?\n\}\);/)?.[0];
assert.ok(directHandler);
let handler;
const calls = [];
runInNewContext(directHandler, {
  ipcMain: { handle: (_channel, fn) => { handler = fn; } },
  nativeRendererCommandAck: async (command, ack) => { calls.push({ ...command, ack }); return command.enabled; },
  writeStartupLog: () => {},
});
for (const invalid of [undefined, null, 1, "true", {}]) assert.equal(await handler(null, invalid), false);
assert.equal(calls.length, 0);
assert.equal(await handler(null, true), true);
assert.equal(await handler(null, false), false);
assert.deepEqual(calls, [
  { type: "setObjectHrtf", enabled: true, ack: "setObjectHrtf" },
  { type: "setObjectHrtf", enabled: false, ack: "setObjectHrtf" },
]);
