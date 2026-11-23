import {readFileSync, writeFileSync} from 'node:fs';
import {dirname, resolve} from 'node:path';
import {createRequire} from 'node:module';

const mobileRequire = createRequire(new URL("../apps/mobile/package.json", import.meta.url));
const expoPackage = mobileRequire.resolve("expo/package.json");
const file = resolve(dirname(expoPackage), "..", "expo-modules-autolinking", "build", "platforms", "apple", "moduleScanner.js");
const source = readFileSync(file, "utf8");
if (!source.includes("timeout: 5000")) throw Error("Expo module scanner timeout target not found");
writeFileSync(file, source.replace("timeout: 5000", "timeout: 30000"));
console.log("Expo module scan timeout: 30s");
