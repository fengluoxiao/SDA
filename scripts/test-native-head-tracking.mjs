import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve, join, dirname, basename } from "node:path";
import { spawnSync } from "node:child_process";
const root=resolve(import.meta.dirname,"..");
const temporary=mkdtempSync(join(tmpdir(),"sda-native-head-tracking-"));
const run=(command,args)=>{const result=spawnSync(command,args,{cwd:root,stdio:"inherit",windowsHide:true});if(result.error)throw result.error;if(result.status!==0)throw Error(command+" failed: "+result.status);};
try {
 const source=resolve(root,"apps/head-tracking-helper/src/orientation.rs").replaceAll("\\","/");
 const harness=join(temporary,"provider.rs"),binary=join(temporary,process.platform==="win32"?"provider.exe":"provider");
 writeFileSync(harness,'mod protocol { #[derive(Clone,Copy,Debug,PartialEq)] pub struct Orientation { pub x:f64,pub y:f64,pub z:f64,pub w:f64 } }\n#[path='+JSON.stringify(source)+'] mod orientation;\n');
 run(process.env.RUSTC??"rustc",["--edition","2021","--test","--cap-lints","allow",harness,"-o",binary]);
 run(binary,[]);
 run(process.execPath,["scripts/test-ios-head-tracking.mjs"]);
 run(process.env.CARGO??"cargo",["test","--manifest-path","apps/native-renderer/Cargo.toml","--locked","--no-default-features","--lib","head_tracking_audio_tests","--","--nocapture"]);
 console.log("Native head tracking startup, transport compatibility, iOS source contracts and measured-HRTF PCM regressions passed; physical headset listening remains separate.");
} finally {
 if(resolve(dirname(temporary))!==resolve(tmpdir())||!basename(temporary).startsWith("sda-native-head-tracking-"))throw Error("Unexpected test cleanup path");
 rmSync(temporary,{recursive:true,force:true});
}
