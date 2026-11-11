const fs=require("node:fs"),path=require("node:path");
const key="sda.scene.preferredFrameRate";
function patchSource(text,kind){
 if(text.includes("SDA_SCENE_FRAME_RATE"))return text;
 const swift=kind==="gl";
 const anchor=swift?"  @objc\n  func drawGL() {":"- (void)_jsThreadUpdate:(CADisplayLink *)displayLink\n{";
 const normalized=text.replace(/\r\n/g,"\n");
 if(!normalized.includes(anchor))throw Error("Unsupported "+kind+" display link source; cannot guarantee scene refresh rate");
 const patch=swift?`
    // SDA_SCENE_FRAME_RATE: only the visible scene requests 60/120; zero restores host defaults.
    let requested = UserDefaults.standard.integer(forKey: "${key}")
    let fps = requested == 120 ? 120 : requested == 60 ? 60 : 0
    if let link = displayLink, link.preferredFrameRateRange.preferred != Float(fps) {
      link.preferredFrameRateRange = CAFrameRateRange(minimum: Float(fps), maximum: Float(fps), preferred: Float(fps))
    }
`:`
  // SDA_SCENE_FRAME_RATE: match JS animation callbacks to the native GL presentation cadence.
  NSInteger requested = [[NSUserDefaults standardUserDefaults] integerForKey:@"${key}"];
  float fps = requested == 120 ? 120 : requested == 60 ? 60 : 0;
  if (displayLink.preferredFrameRateRange.preferred != fps) {
    displayLink.preferredFrameRateRange = fps == 0 ? CAFrameRateRangeDefault : CAFrameRateRangeMake(fps, fps, fps);
  }
`;
 return normalized.replace(anchor,anchor+patch);
}
function patchIOSSceneFrameRates(projectRoot){
 const locations=[["expo-gl","ios/GLView.swift","gl"],["react-native","React/Base/RCTDisplayLink.m","raf"]];
 for(const [module,relative,kind]of locations){
  const file=path.join(path.dirname(require.resolve(module+"/package.json",{paths:[projectRoot]})),relative);
  const before=fs.readFileSync(file,"utf8"),after=patchSource(before,kind);
  if(before!==after)fs.writeFileSync(file,after);
 }
}
module.exports={patchIOSSceneFrameRates,patchSource};
