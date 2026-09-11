import {createRequire} from "node:module";
import {fileURLToPath} from "node:url";
import path from "node:path";
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),"..");
const require=createRequire(path.join(root,"apps/web/package.json"));
const viteRequire=createRequire(require.resolve("vite"));
const {build}=viteRequire("esbuild");
await build({entryPoints:[path.join(root,"apps/web/src/remote-phrtf-core.ts")],bundle:true,format:"esm",platform:"browser",target:"es2022",minify:true,
  outfile:path.join(root,"apps/desktop/remote-web/phrtf-core.mjs"),logLevel:"warning"});

await build({entryPoints:[path.join(root,"apps/web/src/remote-swiper.ts")],bundle:true,format:"esm",platform:"browser",target:"es2022",minify:true,
  outfile:path.join(root,"apps/desktop/remote-web/swiper.mjs"),logLevel:"warning"});

await build({entryPoints:[path.join(root,"apps/web/src/remote-scene.tsx")],bundle:true,format:"esm",platform:"browser",target:"es2022",minify:true,jsx:"automatic",
  define:{"process.env.NODE_ENV":'"production"',"import.meta.env.BASE_URL":'"/"'},
  outfile:path.join(root,"apps/desktop/remote-web/scene-view.mjs"),logLevel:"warning"});
