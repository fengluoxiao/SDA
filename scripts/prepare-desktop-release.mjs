import {readFile,writeFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';import path from 'node:path';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const [platform,arch]=process.argv.slice(2);
if(!['Windows','macOS'].includes(platform)||!['x64','arm64'].includes(arch))throw Error('Expected Windows/macOS and x64/arm64');
const pkg=JSON.parse(await readFile(path.join(root,'apps/desktop/package.json'),'utf8'));
const config=structuredClone(pkg.build);
if(platform==='Windows'&&arch==='arm64'){
 config.win.extraResources=config.win.extraResources.filter(value=>value.to!=='head-tracking-driver');
 config.nsis.include='installer/installer-arm64.nsh';
}
await writeFile(path.join(root,'apps/desktop/release-config.json'),JSON.stringify(config,null,2)+'\n');
console.log(`Desktop release configuration: ${platform} ${arch}, version ${pkg.version}`);
