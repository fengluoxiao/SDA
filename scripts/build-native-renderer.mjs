import { copyFile, cp, mkdir, readdir, readFile, rm } from "node:fs/promises";
import { spawn } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { stageSharedExperiment, verifySharedExperiment } from './stage-shared-render-experiment.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const renderer = join(root, "apps", "native-renderer");
const isWindows = process.platform === "win32";
const exeSuffix = isWindows ? ".exe" : "";
const exeName = `sda-native-renderer${exeSuffix}`;
const targetName = `SdaNativeRenderer${exeSuffix}`;
const targetTriple=process.env.SDA_NATIVE_TARGET;

await new Promise((resolveRun, reject) => {
  const child = spawn(process.env.CARGO ?? "cargo", ["build", "--release", "--locked", "--offline", ...(targetTriple?["--target",targetTriple]:[])], {
    cwd: renderer,
    stdio: "inherit",
    windowsHide: true,
  });
  child.once("error", reject);
  child.once("exit", (code) => code === 0 ? resolveRun() : reject(new Error(`cargo exited with ${code}`)));
});

const destination = join(root, "apps", "desktop", "native-renderer");
await mkdir(destination, { recursive: true });
const source = join(renderer, "target", ...(targetTriple?[targetTriple]:[]), "release", exeName);
const target = join(destination, targetName);
await copyFile(source, target);
if (!isWindows) {
  const { chmod } = await import("node:fs/promises");
  await chmod(target, 0o755);
}
const hrtfDestination = join(destination, "hrtf-assets");
const hrtfSets = [
  "hrtf", "hrtf-dense", "hrtf-raw", "hrtf-dense-raw", "hrtf-d2", "hrtf-d2-dense",
  ...Array.from({ length: 18 }, (_, index) => `hrtf-h${index + 3}-dense`),
  ...Array.from({ length: 18 }, (_, index) => `hrtf-h${index + 3}`),
];
await rm(hrtfDestination, { recursive: true, force: true });
await mkdir(hrtfDestination, { recursive: true });
for (const set of hrtfSets) {
  await cp(join(root, "apps", "web", "public", set), join(hrtfDestination, set), { recursive: true });
}

async function filesUnder(directory, relative = "") {
  const entries = await readdir(join(directory, relative), { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const entryRelative = join(relative, entry.name);
    if (entry.isDirectory()) files.push(...await filesUnder(directory, entryRelative));
    else if (entry.isFile()) files.push(entryRelative);
  }
  return files.sort();
}

async function verifyHrtfStage(set) {
  const source = join(root, "apps", "web", "public", set);
  const staged = join(hrtfDestination, set);
  const sourceFiles = await filesUnder(source);
  const stagedFiles = await filesUnder(staged);
  if (sourceFiles.length === 0 || sourceFiles.join("\0") !== stagedFiles.join("\0")) {
    throw new Error(`HRTF stage mismatch for ${set}`);
  }
  for (const file of sourceFiles) {
    const [sourceBytes, stagedBytes] = await Promise.all([readFile(join(source, file)), readFile(join(staged, file))]);
    if (!sourceBytes.equals(stagedBytes)) throw new Error(`HRTF stage content mismatch for ${set}/${file}`);
  }
}

await Promise.all(hrtfSets.map(verifyHrtfStage));
// Windows and macOS use this same native asset stage. Overlay only the
// explicitly selected experimental dense family; keep standard fallback.
const experiment=stageSharedExperiment(hrtfDestination);
if(experiment) { verifySharedExperiment(hrtfDestination); console.log(`Native experimental renderer: ${experiment.id}`); }
console.log(`Native renderer: ${target} (bundled calibrated HRTF assets verified)`);
