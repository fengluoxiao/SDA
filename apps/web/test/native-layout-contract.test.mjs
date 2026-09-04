import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const app = readFileSync(join(root, "src", "App.tsx"), "utf8");
const declarations = readFileSync(join(root, "src", "vite-env.d.ts"), "utf8");

assert.match(declarations, /nativeRendererLayout\?: \(layout: import\("@sda\/renderer"\)\.LayoutId\)/);
assert.match(app, /const enqueueNative = <T,>\(/);
assert.match(app, /setLayout: async \(layout\) => \{/);
assert.match(app, /desktop\.nativeRendererLayout\?\.\(layout\)/);
assert.match(app, /frame @\$\{samplePos\} \(\$\{entries\.length\} sources\)/);
assert.match(app, /\(\) => desktop\.nativeRendererFrame!\(samplePos, entries\)/);
assert.match(app, /await enqueueNative\(`startAt \$\{origin\}`/);
assert.match(app, /const layoutIdRef = useRef<LayoutId \| "auto">\("auto"\)/);
assert.match(app, /const requestedLayout = layoutIdRef\.current/);
assert.match(app, /if \(requestedLayout !== lid\)/);
assert.match(app, /const resolver = \(labels: readonly string\[\], hasDynamics: boolean\) => \{/);
assert.match(app, /lid === "auto",/);

console.log("native layout web contract tests passed");
