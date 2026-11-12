import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
const root = fileURLToPath(new URL("..", import.meta.url));
const require = createRequire(import.meta.url);
const checks = [
  [require.resolve("typescript/bin/tsc", { paths: [root + "/apps/mobile"] }), "--noEmit", "--pretty", "false", "-p", "apps/mobile/tsconfig.json"],
  ["--test", "apps/mobile/test/retired-ai-cleanup.test.mjs", "apps/mobile/test/object-activity.test.mjs", "apps/mobile/test/cardboard-vr.test.mjs", "apps/mobile/test/scene-performance.test.mjs", "apps/mobile/test/space-visual.test.mjs", "apps/mobile/test/ios-glass-controls.test.mjs", "apps/mobile/test/lyrics.test.mjs", "apps/mobile/test/lyrics-ui.test.mjs", "apps/mobile/test/lyrics-follow.test.mjs", "apps/mobile/test/lyrics-state.test.mjs", "apps/mobile/test/lyrics-mini-return.test.mjs", "apps/mobile/test/lyrics-swipe.test.cjs", "apps/mobile/test/landscape-lyrics-cover.test.cjs", "apps/mobile/test/tablet-landscape-player.test.mjs", "apps/mobile/test/karaoke.test.mjs"],
  ...["test-navigation-preferences", "test-tablet-player-layout", "test-ios-ui", "test-ios-vertical-tab-bar", "test-ios-cutout-side", "test-mobile-production-fixtures", "test-mobile-surface-resume", "test-shared-mini-player-transition"].map(name => ["scripts/" + name + ".mjs"]),
];
for (const args of checks) {
  const result = spawnSync(process.execPath, args, { cwd: root, stdio: "inherit", windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status || 1);
}
console.log("Shared Android/iOS lyrics, navigation, production fixtures and TypeScript checks passed.");
