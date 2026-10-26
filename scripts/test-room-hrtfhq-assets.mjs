import assert from "node:assert/strict";
import {readFileSync,mkdtempSync,rmSync} from "node:fs";
import {gunzipSync} from "node:zlib";
import {createHash} from "node:crypto";
import {execFileSync} from "node:child_process";
import {tmpdir} from "node:os";
import {join,dirname,resolve,basename} from "node:path";
const bundle=JSON.parse(gunzipSync(readFileSync("assets/hrtf/apple-hrtfhq-v1/room-receiver.json.gz")));
const live=JSON.parse(readFileSync("assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json","utf8"));
assert.equal(bundle.manifest.profileId,live.sharedRenderProfile);assert.equal(bundle.manifest.preparation,"native-shared-clarity-v1");
assert.equal(bundle.manifest.positions.length,542);assert.equal(Object.keys(bundle.files).length,542);
const hash=createHash("sha256");
for(const [i,entry]of bundle.manifest.positions.entries()){
 assert.equal(entry.azimuth,live.positions[i].azimuth);assert.equal(entry.elevation,live.positions[i].elevation);
 const bytes=Buffer.from(bundle.files[entry.dry],"base64");assert.equal(bytes.length,640*2*4);hash.update(bytes);
 for(let at=0;at<bytes.length;at+=4)assert(Number.isFinite(bytes.readFloatLE(at)));
}
assert.equal(hash.digest("hex"),bundle.manifest.receiverSha256);
const catalog=JSON.parse(readFileSync("apps/desktop/builtin-rooms/catalog.json","utf8"));
for(const entry of catalog.profiles){assert.equal(entry.summary.simulation.hrtfSha256,bundle.manifest.receiverSha256);assert.equal(entry.summary.simulation.hrtfProfile,bundle.manifest.profileId);}
if(process.env.SDA_ROOM_REFERENCE){
 const temporary=mkdtempSync(join(tmpdir(),"sda-room-native-"));
 try{
  execFileSync(process.env.SDA_ROOM_REFERENCE,["export",resolve("assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json"),temporary]);
  const actual=JSON.parse(readFileSync(join(temporary,"hrtf-set.json"),"utf8"));
  for(const entry of actual.positions)assert.deepEqual(readFileSync(join(temporary,entry.dry)),Buffer.from(bundle.files[entry.dry],"base64"),"committed receiver must equal the actual native loader preparation");
 }finally{
  assert.equal(dirname(temporary),resolve(tmpdir()));assert(basename(temporary).startsWith("sda-room-native-"));rmSync(temporary,{recursive:true,force:true});
 }
}
console.log("HRTFHQ receiver: 542 native-prepared directions, finite 640-tap stereo data, digest and all room identities passed");
