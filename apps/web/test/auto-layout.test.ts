import assert from "node:assert/strict";
import {formatAutoLayout,resolveAutoLayout,uses360RaLowerLayer} from "../src/auto-layout";
for(const codec of ["mpegh","mha1","mhm1"]){
  assert.equal(uses360RaLowerLayer(codec),true);
  assert.equal(resolveAutoLayout(["L","R"],false,codec),"360RA-13");
  assert.equal(resolveAutoLayout(["Obj_0"],true,codec),"360RA-13");
}
assert.equal(uses360RaLowerLayer("eac3"),false);
assert.equal(resolveAutoLayout(["L","R"],false,"alac",true),"7.1.4");
assert.notEqual(resolveAutoLayout(["L","R"],false,"alac",false),"7.1.4");
for(const codec of ["eac3","truehd","ac4"]){
  assert.equal(resolveAutoLayout(["WideLeft","TopMiddleLeft","TopRearLeft"],true,codec),"7.1.4");
  assert.equal(resolveAutoLayout(["L","R","C"],false,codec),"7.1.4");
}
assert.equal(formatAutoLayout("bwf"),undefined);
assert.equal(resolveAutoLayout(["L","R","C","Ls","Rs"],false,"dts"),"5.1");
assert.equal(resolveAutoLayout(["L","R","C","LFE","Ls","Rs"],false,"flac"),"5.1");
assert.equal(resolveAutoLayout(["L","R","C","LFE","Lb","Rb","Ls","Rs"],false,"flac"),"7.1");
assert.equal(resolveAutoLayout(["L","R","C","LFE","Ls","Rs","Tfl","Tfr"],false,"flac"),"5.1.2");
console.log("Format defaults: Dolby 7.1.4, MPEG-H 360RA-13, and discrete beds preserve their authored layout");
