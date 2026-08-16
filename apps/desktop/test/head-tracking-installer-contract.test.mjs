import assert from "node:assert/strict";
import { createHash, X509Certificate } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const desktopDir = join(import.meta.dirname, "..");
const rootDir = join(desktopDir, "..", "..");
const installer = readFileSync(join(desktopDir, "installer", "installer.nsh"), "utf8");
const setupScript = readFileSync(
  join(desktopDir, "installer", "install-head-tracking-driver.ps1"),
  "utf8",
);
const desktopPackage = JSON.parse(readFileSync(join(desktopDir, "package.json"), "utf8"));
const driverPackageDir = join(rootDir, "apps", "head-tracking-driver", "package");
const driverInf = readFileSync(join(driverPackageDir, "SdaAirPodsL2cap.inf"), "utf8");

assert.equal(desktopPackage.build.nsis.oneClick, false);
assert.equal(desktopPackage.build.nsis.allowToChangeInstallationDirectory, true);
assert.equal(desktopPackage.build.nsis.include, "installer/installer.nsh");
assert.ok(
  desktopPackage.build.win.extraResources.some(
    ({ from, to }) => from === "head-tracking-driver" && to === "head-tracking-driver",
  ),
);

assert.match(installer, /Enable Windows TestSigning/);
assert.match(installer, /Install the SDA AirPods head-tracking driver/);
assert.match(installer, /\$\{NSD_Uncheck\} \$SdaEnableTestSigningCheckbox/);
assert.match(installer, /\$\{NSD_Uncheck\} \$SdaInstallDriverCheckbox/);
assert.match(installer, /-EnableTestSigning/);
assert.match(installer, /-InstallDriver/);
assert.match(installer, /ExecWait/);

assert.match(setupScript, /\[switch\]\$EnableTestSigning/);
assert.match(setupScript, /\[switch\]\$InstallDriver/);
assert.match(setupScript, /-Verb RunAs/);
assert.match(setupScript, /ProgramData.*SDA\\Logs/);
assert.match(
  setupScript,
  /887FBB9BFF2D202DA0E0D828FEF7C0CA8B422193424F8C658E6ADB50A37EBFB5/,
);
assert.doesNotMatch(setupScript, /Restart-Computer|shutdown\.exe/i);

const expectedPackageSha256 = new Map([
  ["SdaAirPodsL2cap.cat", "C83FD956F79553F0ADDB91E17875FA515B421821C2638D8AEA85208A7B38AA11"],
  ["SdaAirPodsL2cap.cer", "887FBB9BFF2D202DA0E0D828FEF7C0CA8B422193424F8C658E6ADB50A37EBFB5"],
  ["SdaAirPodsL2cap.inf", "572F9A62B7D99E7A13DFC867EE24E8A6B084A51F6D5CEA92153196A8E0416401"],
  ["SdaAirPodsL2cap.sys", "C85FA809E0FE0B2250748214F154F559EAAB81798381BA1F1CDE254A463172AE"],
]);
for (const [name, expectedSha256] of expectedPackageSha256) {
  const packagePath = join(driverPackageDir, name);
  assert.ok(existsSync(join(driverPackageDir, name)), `${name} is missing from the driver package`);
  assert.equal(
    createHash("sha256").update(readFileSync(packagePath)).digest("hex").toUpperCase(),
    expectedSha256,
    `${name} must remain byte-for-byte identical to the signed driver package`,
  );
}

const certificate = new X509Certificate(
  readFileSync(join(driverPackageDir, "SdaAirPodsL2cap.cer")),
);
assert.equal(
  certificate.fingerprint256.replaceAll(":", ""),
  "887FBB9BFF2D202DA0E0D828FEF7C0CA8B422193424F8C658E6ADB50A37EBFB5",
);

const allowedPids = ["200E", "2014", "2024", "2013", "2019", "201B", "200A", "201F"];
const hardwareIds = [...driverInf.matchAll(/^.+?=.+?,(BTHENUM\\[^\r\n]+)$/gm)].map(
  ([, hardwareId]) => hardwareId,
);
assert.equal(hardwareIds.length, allowedPids.length);
for (const [index, hardwareId] of hardwareIds.entries()) {
  assert.match(hardwareId, /_VID&0001004C_PID&[0-9A-F]{4}$/);
  assert.ok(hardwareId.endsWith(`_PID&${allowedPids[index]}`));
}

console.log("head tracking installer contract tests passed");
