import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
const root=resolve(import.meta.dirname,"..");
const result=spawnSync(process.env.CARGO??"cargo",["test","--manifest-path","apps/native-renderer/Cargo.toml","--locked","--no-default-features","--lib","stereo_fifo::tests"],{cwd:root,stdio:"inherit",windowsHide:true});
if(result.error)throw result.error;if(result.status!==0)process.exit(result.status||1);
console.log("Native output FIFO: payload preservation, flush/seek boundaries and silence fill passed.");
