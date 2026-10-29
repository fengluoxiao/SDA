import { copyFile, cp, mkdir, readdir, readFile, rm, lstat, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { gunzipSync } from "node:zlib";
import { spawn, execFileSync } from "node:child_process";
import { dirname, join, resolve, relative, isAbsolute } from "node:path";
import { fileURLToPath } from "node:url";
import { stageSharedExperiment, verifySharedExperiment } from './stage-shared-render-experiment.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const renderer = join(root, "apps", "native-renderer");
const isWindows = process.platform === "win32";
const exeSuffix = isWindows ? ".exe" : "";
const exeName = `sda-native-renderer${exeSuffix}`;
const targetName = `SdaNativeRenderer${exeSuffix}`;
const targetTriple=process.env.SDA_NATIVE_TARGET;
process.env.MACINDECODE_AC4_SPEC_DIR ??= join(root,'tmp','MacinDecode-AC4-Core','spec');
process.env.SDA_SOURCE_COMMIT ??= execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim();

await new Promise((resolveRun, reject) => {
  const child = spawn(process.env.CARGO ?? "cargo", ["build", "--release", "--locked", "--offline", "--bin", "sda-native-renderer", "--example", "room_hrtf", ...(targetTriple?["--target",targetTriple]:[])], {
    cwd: renderer,
    stdio: "inherit",
    windowsHide: true,
  });
  child.once("error", reject);
  child.once("exit", (code) => code === 0 ? resolveRun() : reject(new Error(`cargo exited with ${code}`)));
});

const destination = join(root, "apps", "desktop", "native-renderer");
await mkdir(destination, { recursive: true });
const cargoTarget = process.env.CARGO_TARGET_DIR ? resolve(renderer, process.env.CARGO_TARGET_DIR) : join(renderer, "target");
const source = join(cargoTarget, ...(targetTriple?[targetTriple]:[]), "release", exeName);
const target = join(destination, targetName);
await copyFile(source, target);
const roomHelper = join(destination, `SdaRoomReference${exeSuffix}`);
await copyFile(join(cargoTarget, ...(targetTriple?[targetTriple]:[]), "release", "examples", `room_hrtf${exeSuffix}`), roomHelper);
if (!isWindows) {
  const { chmod } = await import("node:fs/promises");
  await chmod(target, 0o755);
  await chmod(roomHelper, 0o755);
}
const hrtfDestination = join(destination, "hrtf-assets");
const hrtfSets = [
  "hrtf", "hrtf-dense", "hrtf-raw", "hrtf-dense-raw", "hrtf-d2", "hrtf-d2-dense",
  ...Array.from({ length: 18 }, (_, index) => `hrtf-h${index + 3}-dense`),
  ...Array.from({ length: 18 }, (_, index) => `hrtf-h${index + 3}`),
];
const assetRelative = relative(root, resolve(hrtfDestination));
if (assetRelative.startsWith("..") || isAbsolute(assetRelative) || (await lstat(hrtfDestination).catch(() => null))?.isSymbolicLink()) {
  throw new Error("Refusing to replace native HRTF assets outside this workspace");
}
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
await cp(join(root,'apps/mobile/assets/hrtf-restored/hrtf'),join(hrtfDestination,'hrtf-ku100-reference'),{recursive:true});
await cp(join(root,'apps/mobile/ios-experiments/fullheight-combined/hrtf-dense'),join(hrtfDestination,'hrtf-ku100-reference-dense'),{recursive:true});
const experiment=stageSharedExperiment(hrtfDestination);
if(experiment) { verifySharedExperiment(hrtfDestination); console.log(`Native experimental renderer: ${experiment.id}`); }
// Commit the native-prepared receiver data separately, so cross-target builds
// never need to execute a target-platform helper on the build host.
const receiver = JSON.parse(gunzipSync(await readFile(join(root,"assets/hrtf/apple-hrtfhq-v1/room-receiver.json.gz")),{maxOutputLength:16*1024*1024}));
if(receiver.manifest?.profileId!=="apple-hrtfhq-542-v1"||receiver.manifest.positions?.length!==542)throw new Error("Invalid prepared room receiver");
const receiverHash=createHash("sha256");
const roomReceiverRoot=join(destination,"room-hrtfhq");await mkdir(roomReceiverRoot,{recursive:true});
for(const entry of receiver.manifest.positions){
  if(!/^direction-\d{3}\.f32$/.test(entry.dry))throw new Error("Invalid room receiver path");
  const data=Buffer.from(receiver.files[entry.dry],"base64");
  if(data.length!==640*2*4)throw new Error("Invalid room receiver length");
  receiverHash.update(data);
  await writeFile(join(roomReceiverRoot,entry.dry),data);
}
if(receiverHash.digest("hex")!==receiver.manifest.receiverSha256)throw new Error("Prepared room receiver checksum mismatch");
await writeFile(join(roomReceiverRoot,"hrtf-set.json"),JSON.stringify(receiver.manifest,null,2)+"\n");
console.log(`Native renderer: ${target} (bundled calibrated HRTF assets verified)`);

const bytes = await readFile(target);
if(!bytes.includes(Buffer.from('bedExportReady')))throw new Error('Native renderer lacks offline bed-balance export');
const sourceCommit = execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim();
if (!bytes.includes(Buffer.from(sourceCommit)) || !bytes.includes(Buffer.from('front-direct-mirror-lowband-v2'))) throw new Error('Staged renderer does not match the current source revision and stereo calibration');
await writeFile(join(destination,'build-info.json'),JSON.stringify({sourceCommit,target:targetTriple??process.platform,sha256:createHash('sha256').update(bytes).digest('hex'),hrtfProfile:experiment?.id??null,bedExportProtocol:1,roomReferenceSha256:createHash('sha256').update(await readFile(roomHelper)).digest('hex'),stereoBassPreservation:'front-direct-mirror-lowband-v2'},null,2)+'\n');
