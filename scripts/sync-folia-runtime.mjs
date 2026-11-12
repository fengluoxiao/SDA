import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { createRequire } from "node:module";
const require = createRequire(import.meta.url), ts = require("typescript");
const root = resolve(import.meta.dirname, ".."), directory = resolve(root, "vendor/folia");
const source = readFileSync(resolve(directory, "upstream/LatentBackground.tsx.source"), "utf8");
const parsed = ts.createSourceFile("LatentBackground.tsx", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
const needed = new Set(["MAX_SHADER_PIXELS", "PAUSED_SPEED_SCALE", "PAUSE_EASE_MS", "normalizeAudio", "clampShaderSpeed", "clampAudioAmount", "easeTowards", "resolveLatentBroadbandEnergy", "resolveLatentOnsetPulse", "resolveLatentBeatSpeedTarget", "resolveLatentBeatFxPulse", "resolveLatentAudioSpeedTarget", "resolveLatentShaderSpeed", "resolveLatentShaderColors"]);
const blocks = [];
for (const node of parsed.statements) {
 if (!ts.isVariableStatement(node)) continue;
 const names = node.declarationList.declarations.map(d => d.name.getText(parsed));
 if (!names.some(n => needed.has(n))) continue;
 if (names.some(n => !needed.has(n))) throw Error("Mixed upstream declarations require explicit review");
 const text = node.getText(parsed); blocks.push(text.startsWith("export ") ? text : "export " + text);
 names.forEach(n => needed.delete(n));
}
if (needed.size) throw Error("Missing upstream declarations: " + [...needed].join(","));
const types = readFileSync(resolve(directory, "upstream/types.ts.source"), "utf8"), tree = ts.createSourceFile("types.ts", types, ts.ScriptTarget.Latest, true);
let defaults;
for (const node of tree.statements) if (ts.isVariableStatement(node)) for (const d of node.declarationList.declarations) if (d.name.getText(tree) === "DEFAULT_LATENT_BACKGROUND_TUNING") defaults = d.initializer.getText(tree);
if (!defaults) throw Error("Missing Folia tuning defaults");
const output = '// Generated from Folia AGPL-3.0 source; see upstream.json and LICENSE. Do not edit function bodies.\n' +
 'type Theme = { backgroundColor: string; primaryColor: string; secondaryColor: string; accentColor: string };\n' +
 'type LatentBackgroundColorSource = "cover-only" | "cover-theme";\n' +
 'export const DEFAULT_LATENT_BACKGROUND_TUNING = ' + defaults + ' as const;\n' + blocks.join('\n') + '\n';
const target = resolve(directory, "latent.generated.ts");
if (process.argv.includes("--check")) { if (readFileSync(target,"utf8") !== output) throw Error("Folia generated runtime differs from upstream"); }
else writeFileSync(target,output);
console.log("Folia runtime: original defaults and 14 declarations verified/extracted.");

const promptSource=readFileSync(resolve(directory,"upstream/aiThemePrompts.ts.source"),"utf8"), promptTree=ts.createSourceFile("aiThemePrompts.ts",promptSource,ts.ScriptTarget.Latest,true);
let prompt;
for(const node of promptTree.statements)if(ts.isVariableStatement(node))for(const d of node.declarationList.declarations)if(d.name.getText(promptTree)==="THEME_GENERATION_PROMPT_PREFIX")prompt=d.initializer.getText(promptTree);
if(!prompt)throw Error("Folia theme prompt missing");
const promptOutput="// Generated from Folia AGPL-3.0 source; see upstream.json and LICENSE.\nexport const FOLIA_THEME_PROMPT = "+prompt+";\n";
const promptPath=resolve(directory,"theme-prompt.generated.ts");
if(process.argv.includes("--check")){if(readFileSync(promptPath,"utf8")!==promptOutput)throw Error("Folia theme prompt differs from upstream");}else writeFileSync(promptPath,promptOutput);
