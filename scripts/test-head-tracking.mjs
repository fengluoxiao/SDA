import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { build } from "esbuild";

const root = resolve(import.meta.dirname, "..");
const temporaryDirectory = await mkdtemp(join(tmpdir(), "sda-head-tracking-tests-"));
const bundledTests = [
  "packages/renderer/test/head-pose.test.mjs",
  "packages/renderer/test/head-tracking-message.test.mjs",
  "packages/renderer/test/binaural-lifecycle-health.test.mjs",
  "apps/web/test/head-tracking-telemetry.test.ts",
  "apps/web/test/head-tracking-session.test.ts",
];
const directTests = [
  "packages/renderer/test/head-tracking-object-schedule.test.mjs",
  "apps/desktop/test/head-tracking-helper-contract.test.mjs",
  "apps/desktop/test/head-tracking-installer-contract.test.mjs",
  "apps/web/test/head-tracking-replay-contract.test.mjs",
];

function runNode(script) {
  return new Promise((resolveRun, reject) => {
    const child = spawn(process.execPath, [script], { cwd: root, stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", (code) => {
      if (code === 0) resolveRun();
      else reject(new Error(`${script} exited with ${code}`));
    });
  });
}

try {
  for (const relativePath of bundledTests) {
    const outputPath = join(temporaryDirectory, `${basename(relativePath)}.bundle.mjs`);
    await build({
      entryPoints: [join(root, relativePath)],
      outfile: outputPath,
      bundle: true,
      format: "esm",
      platform: "node",
      sourcemap: "inline",
    });
    await runNode(outputPath);
  }

  for (const relativePath of directTests) {
    await runNode(join(root, relativePath));
  }
} finally {
  await rm(temporaryDirectory, { recursive: true, force: true });
}
