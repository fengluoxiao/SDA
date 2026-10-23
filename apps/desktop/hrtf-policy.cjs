const path=require("node:path");
const BUNDLED_HRTF_PATTERN=/^hrtf(?:-[a-z0-9]+)*\/(?:hrtf-set\.json|triangle-grid\.json|direction-\d{3}(?:-(?:dry|wet))?\.f32|azm?\d+_elm?\d+_(?:dry|wet)\.f32)$/;
const NATIVE_HRTF_SET_PATTERN=/^hrtf(?:-dense(?:-raw)?|-raw|-ku100-reference(?:-dense)?|-d2(?:-dense)?|-h(?:[3-9]|1[0-9]|20)(?:-dense)?)?$/;
function bundledHrtfRoot(set,executable,webRoot) {
 if(executable) {
  // Never silently read an older web copy when the sidecar family is missing.
  return path.join(path.dirname(executable),"hrtf-assets");
 }
 return webRoot;
}
module.exports={BUNDLED_HRTF_PATTERN,NATIVE_HRTF_SET_PATTERN,bundledHrtfRoot};
