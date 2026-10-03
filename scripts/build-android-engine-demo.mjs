import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const crate = resolve(root, 'crates/sda-native');
const android = resolve(root, 'apps/android-engine-demo/android');
const env = { ...process.env };
if (!env.ANDROID_NDK_HOME || !env.JAVA_HOME || !env.ANDROID_HOME) {
  throw new Error('Set ANDROID_NDK_HOME, JAVA_HOME and ANDROID_HOME before building.');
}
env.MACINDECODE_AC4_SPEC_DIR ??= resolve(root, 'tmp/MacinDecode-AC4-Core/spec');
const toolchain = process.env.SDA_RUST_TOOLCHAIN ?? '1.98.0';
const run = (command, args, cwd) => {
  const result = spawnSync(command, args, {
    cwd, env, stdio: 'inherit', shell: process.platform === 'win32',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed (${result.status})`);
};
run('cargo', [`+${toolchain}`, 'ndk', '-t', 'x86_64', '--platform', '26', 'build', '--release', '--no-default-features'], crate);
const metadata = spawnSync('cargo', [`+${toolchain}`, 'metadata', '--no-deps', '--format-version', '1'], {
  cwd: crate, env, encoding: 'utf8', shell: process.platform === 'win32',
});
if (metadata.status !== 0) throw new Error(metadata.stderr || 'cargo metadata failed');
const target = JSON.parse(metadata.stdout).target_directory;
const source = resolve(target, 'x86_64-linux-android/release/libsda_native.so');
const staged = resolve(android, 'app/src/main/jniLibs/x86_64/libsda_native.so');
mkdirSync(dirname(staged), { recursive: true });
copyFileSync(source, staged);
const hash = file => createHash('sha256').update(readFileSync(file)).digest('hex');
if (hash(staged) !== hash(source)) throw new Error('Native library staging mismatch');
run(process.env.SDA_GRADLE ?? 'gradle', ['assembleDebug', '--no-daemon', '--console=plain'], android);
const apk = resolve(android, 'app/build/outputs/apk/debug/app-debug.apk');
const manifest = { contract: 'ndk-f32-v1', abi: 'x86_64', nativeSha256: hash(staged), apkSha256: hash(apk), apk };
writeFileSync(resolve(android, 'app/build/outputs/apk/debug/build-verification.json'), JSON.stringify(manifest, null, 2) + '\n');
console.log(JSON.stringify(manifest, null, 2));
