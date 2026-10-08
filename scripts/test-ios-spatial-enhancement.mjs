import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const read=p=>readFileSync(p,'utf8');
const app=read('apps/mobile/App.tsx');
assert.match(app,/spatialEnhancementEnabled: false/);
assert.match(app,/spatialEnhancementEnabled: settings\.spatialEnhancementEnabled === true/);
assert.match(app,/setSpatialEnhancement=\{this\.setSpatialEnhancement\}/);
assert.match(app,/this\.state\.systemSpatial360RAActive\) return/);
for(const path of ['apps/mobile/src/IOSNativeSettings.tsx','apps/mobile/src/IOSPlayer.tsx']) {
 const ui=read(path);assert.ok(ui.includes('空间层增强 · +2/+6'));assert.ok(ui.includes('p.setSpatialEnhancement'));
 assert.ok(ui.includes('p.busy || p.systemSpatial360RAActive'));
}
const player=read('apps/mobile/modules/sda-core/ios/SdaPlayer.swift');
assert.match(player,/handle != nil && systemSpatial == nil/);
assert.match(player,/prefs\.set\(enabled, forKey:"sda\.spatialEnhancement"\)/);
assert.match(player,/command\("spatialEnhancement", \["enabled":s\["spatialEnhancementEnabled"\]!\]\)/);
assert.ok(read('apps/mobile/modules/sda-core/ios/SdaModule.swift').includes('Function("setSpatialEnhancement")'));
assert.ok(read('crates/sda-native/src/ios.rs').includes('"spatialEnhancement" => e.set_spatial_enhancement'));
assert.ok(read('crates/sda-native/src/lib.rs').includes('Command::SetSpatialEnhancement { enabled }'));
console.log('iOS spatial enhancement checks passed: default-off, persisted, restored, native recipe and system-route bypass.');
