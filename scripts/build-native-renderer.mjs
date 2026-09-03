import { copyFile, cp, mkdir, rm } from "node:fs/promises";
import { spawn } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const renderer = join(root, "apps", "native-renderer");

if (process.platform !== "win32") {
  console.log("Native renderer: skipped on non-Windows host");
  process.exit(0);
}

await new Promise((resolveRun, reject) => {
  const child = spawn(process.env.CARGO ?? "cargo", ["build", "--release", "--locked", "--offline"], {
    cwd: renderer,
    stdio: "inherit",
    windowsHide: true,
  });
  child.once("error", reject);
  child.once("exit", (code) => code === 0 ? resolveRun() : reject(new Error(`cargo exited with ${code}`)));
});

const destination = join(root, "apps", "desktop", "native-renderer");
await mkdir(destination, { recursive: true });
const source = join(renderer, "target", "release", "sda-native-renderer.exe");
const target = join(destination, "SdaNativeRenderer.exe");
await copyFile(source, target);
const hrtfDestination = join(destination, "hrtf-assets");
await rm(hrtfDestination, { recursive: true, force: true });
await mkdir(hrtfDestination, { recursive: true });
for (const set of ["hrtf", "hrtf-dense", "hrtf-d2", ...Array.from({ length: 18 }, (_, index) => `hrtf-h${index + 3}`)]) {
  await cp(join(root, "apps", "web", "public", set), join(hrtfDestination, set), { recursive: true });
}
console.log(`Native renderer: ${target} (bundled calibrated HRTF assets)`);
