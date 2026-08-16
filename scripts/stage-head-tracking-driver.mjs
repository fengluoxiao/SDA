import { createHash, X509Certificate } from "node:crypto";
import { copyFile, mkdir, readFile, rm } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const EXPECTED_CERTIFICATE_SHA256 =
  "887FBB9BFF2D202DA0E0D828FEF7C0CA8B422193424F8C658E6ADB50A37EBFB5";
const EXPECTED_PACKAGE_SHA256 = new Map([
  ["SdaAirPodsL2cap.cat", "C83FD956F79553F0ADDB91E17875FA515B421821C2638D8AEA85208A7B38AA11"],
  ["SdaAirPodsL2cap.cer", EXPECTED_CERTIFICATE_SHA256],
  ["SdaAirPodsL2cap.inf", "572F9A62B7D99E7A13DFC867EE24E8A6B084A51F6D5CEA92153196A8E0416401"],
  ["SdaAirPodsL2cap.sys", "C85FA809E0FE0B2250748214F154F559EAAB81798381BA1F1CDE254A463172AE"],
]);
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

for (const source of inputs.slice(0, 4)) {
  const name = basename(source);
  const actualSha256 = createHash("sha256").update(await readFile(source)).digest("hex").toUpperCase();
  const expectedSha256 = EXPECTED_PACKAGE_SHA256.get(name);
  if (actualSha256 !== expectedSha256) {
    throw new Error(
      `${name} SHA-256 mismatch: expected ${expectedSha256}, got ${actualSha256}`,
    );
  }
}

await rm(outputDir, { recursive: true, force: true });
await mkdir(outputDir, { recursive: true });
for (const source of inputs) {
  await copyFile(source, join(outputDir, basename(source)));
}

console.log(`Staged ${inputs.length} verified head-tracking driver files in ${outputDir}`);
