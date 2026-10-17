import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const read=p=>readFileSync(p,'utf8');
const app=read('apps/mobile/App.tsx');
assert.match(app,/spatialEnhancementEnabled: false/);
assert.match(app,/spatialEnhancementEnabled: settings\.spatialEnhancementEnabled === true/);
assert.match(app,/setSpatialEnhancement=\{this\.setSpatialEnhancement\}/);
assert.match(app,/this\.state\.systemSpatial360RAActive\) return/);
for(const path of ['apps/mobile/src/IOSNativeSettings.tsx','apps/mobile/src/IOSPlayer.tsx']) {
 const ui=read(path);assert.ok(ui.includes('空间平衡'));assert.ok(ui.includes('p.setSpatialEnhancement'));
 assert.ok(ui.includes('p.busy || p.systemSpatial360RAActive'));
}
const player=read('apps/mobile/modules/sda-core/ios/SdaPlayer.swift');
assert.match(player,/handle != nil && systemSpatial == nil/);
assert.match(player,/prefs\.set\(enabled, forKey:"sda\.spatialEnhancement"\)/);
assert.match(player,/command\("spatialEnhancement", \["enabled":s\["spatialEnhancementEnabled"\]!\]\)/);
assert.ok(read('apps/mobile/modules/sda-core/ios/SdaModule.swift').includes('Function("setSpatialEnhancement")'));
assert.ok(read('crates/sda-native/src/ios.rs').includes('"spatialEnhancement" => e.set_spatial_enhancement'));
assert.ok(read('crates/sda-native/src/lib.rs').includes('Command::SetSpatialEnhancement { enabled }'));
assert.ok(player.includes('prefs.set(true,forKey:"sda.spatialEnhancement")'));
console.log('iOS spatial enhancement checks passed: shared-profile default, persisted override, native recipe and system-route bypass.');

const recipe=read("apps/native-renderer/src/lib.rs").split("pub fn set_spatial_enhancement")[1].split("fn set_output_volume")[0];
assert.ok(recipe.includes("self.set_master_preamp_db(0.0)"));
assert.ok(recipe.includes("if enabled { 1.5 } else { 0.0 }"));
assert.ok(recipe.includes("self.spatial_layer.main_restore_db = 0.0"));
assert.ok(read("apps/native-renderer/src/spatial_cues.rs").includes("taper_residual_tail(&mut p, 20.0)"));
for (const path of ["apps/mobile/src/IOSNativeSettings.tsx","apps/mobile/src/IOSPlayer.tsx"]) assert.ok(!read(path).includes("−0.75 dB"));
