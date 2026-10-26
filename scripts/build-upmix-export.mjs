import {build} from "esbuild";
import {fileURLToPath} from "node:url";
const root=fileURLToPath(new URL("../",import.meta.url));
await build({entryPoints:[root+"packages/player/src/upmix-export.worker.ts"],bundle:true,platform:"node",format:"cjs",target:"node22",outfile:root+"apps/desktop/upmix-export-worker.cjs",logLevel:"warning"});
