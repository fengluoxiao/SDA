import { X509Certificate } from "node:crypto";
import { copyFile, mkdir, readFile, rm } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const EXPECTED_CERTIFICATE_SHA256 =
  "887FBB9BFF2D202DA0E0D828FEF7C0CA8B422193424F8C658E6ADB50A37EBFB5";
const root = dirname(dirname(fileURLToPath(import.meta.url)));
const packageDir = join(root, "apps", "head-tracking-driver", "package");
const installerDir = join(root, "apps", "desktop", "installer");
const outputDir = join(root, "apps", "desktop", "head-tracking-driver");
const inputs = [
  join(packageDir, "SdaAirPodsL2cap.cer"),
  join(packageDir, "SdaAirPodsL2cap.inf"),
  join(packageDir, "SdaAirPodsL2cap.cat"),
  join(packageDir, "SdaAirPodsL2cap.sys"),
  join(installerDir, "install-head-tracking-driver.ps1"),
];

const certificate = new X509Certificate(await readFile(inputs[0]));
const actualFingerprint = certificate.fingerprint256.replaceAll(":", "").toUpperCase();
if (actualFingerprint !== EXPECTED_CERTIFICATE_SHA256) {
  throw new Error(
    `Driver certificate SHA-256 mismatch: expected ${EXPECTED_CERTIFICATE_SHA256}, got ${actualFingerprint}`,
  );
}

await rm(outputDir, { recursive: true, force: true });
await mkdir(outputDir, { recursive: true });
for (const source of inputs) {
  await copyFile(source, join(outputDir, basename(source)));
}

console.log(`Staged ${inputs.length} verified head-tracking driver files in ${outputDir}`);
