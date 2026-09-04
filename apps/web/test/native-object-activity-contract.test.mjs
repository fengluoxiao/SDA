import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const app = readFileSync(join(root, "src", "App.tsx"), "utf8");
const declarations = readFileSync(join(root, "src", "vite-env.d.ts"), "utf8");

assert.match(declarations, /interface NativeRendererObjectActivity/);
assert.match(declarations, /onNativeRendererObjectActivity\?: \(callback:/);
assert.match(app, /onObjectActivity: \(callback\) => desktop\.onNativeRendererObjectActivity/);
assert.match(app, /if \(!ownsNativeSession\(\) \|\| !Array\.isArray\(activity\?\.ids\)\) return;/);
assert.match(app, /callback\(activity\.ids\);/);
assert.match(app, /setSoundingObjectIds\(sounding\)/);

console.log("native object activity web contract tests passed");
