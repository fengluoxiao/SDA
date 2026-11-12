import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
const root=resolve(import.meta.dirname,"..");
const args=["test","--manifest-path","apps/native-renderer/Cargo.toml","--locked","--no-default-features","--lib","visual_"];
const result=spawnSync(process.env.CARGO??"cargo",args,{cwd:root,stdio:"inherit",windowsHide:true});
if(result.error)throw result.error;if(result.status!==0)process.exit(result.status||1);
console.log("Folia passive visual meters: consumed PCM, silence, frequency response, anti-phase and unchanged samples passed.");
