import { existsSync } from "node:fs";
import { copyFile, mkdir } from "node:fs/promises";
import { spawn } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const helper = join(root, "apps", "head-tracking-helper");
const cargo = process.env.CARGO ?? "cargo";
const rustc = process.env.RUSTC ?? "rustc";

if (process.platform !== "win32") {
  console.log("Windows AirPods helper: skipped on non-Windows host");
  process.exit(0);
}

function output(command, args) {
  return new Promise((resolveOutput, reject) => {
    let value = "";
    const child = spawn(command, args, { windowsHide: true });
    child.stdout.on("data", (chunk) => { value += chunk; });
    child.once("error", reject);
    child.once("exit", (code) => code === 0
      ? resolveOutput(value.trim())
      : reject(new Error(`${command} exited with ${code}`)));
  });
}

const pathParts = [];
if (process.platform === "win32") {
  const sysroot = await output(rustc, ["--print", "sysroot"]);
  const selfContained = join(sysroot, "lib", "rustlib", "x86_64-pc-windows-gnu", "bin", "self-contained");
  if (existsSync(selfContained)) pathParts.push(selfContained);
  // Recent rustup GNU toolchains include dlltool but may omit its assembler.
  if (existsSync("C:\\msys64\\mingw64\\bin\\as.exe")) pathParts.unshift("C:\\msys64\\mingw64\\bin");
}
const cargoEnvironment = {
  ...process.env,
  Path: [...pathParts, process.env.Path ?? process.env.PATH ?? ""].filter(Boolean).join(";"),
};

await new Promise((resolveRun, reject) => {
  const child = spawn(cargo, ["build", "--release", "--locked", "--offline"], {
    cwd: helper,
    env: cargoEnvironment,
    stdio: "inherit",
    windowsHide: true,
  });
  child.once("error", reject);
  child.once("exit", (code) => code === 0 ? resolveRun() : reject(new Error(`cargo exited with ${code}`)));
});

const destination = join(root, "apps", "desktop", "head-tracking-helper");
await mkdir(destination, { recursive: true });
await Promise.all([
  copyFile(join(helper, "target", "release", "sda-airpods-head-tracking.exe"), join(destination, "SdaAirPodsHeadTracking.exe")),
  copyFile(join(helper, "LICENSE"), join(destination, "LICENSE.txt")),
]);
console.log(`Windows AirPods helper: ${join(destination, "SdaAirPodsHeadTracking.exe")}`);
