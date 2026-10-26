import {mkdir,readFile,copyFile,chmod,writeFile,access} from 'node:fs/promises';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createRequire} from 'node:module';
import {spawn} from 'node:child_process';
const require=createRequire(import.meta.url),root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const architectures={0:'ia32',1:'x64',2:'armv7l',3:'arm64'};
const execute=(exe,args,env=process.env)=>new Promise((done,fail)=>{const child=spawn(exe,args,{stdio:'inherit',windowsHide:true,env});child.once('error',fail);child.once('exit',code=>code===0?done():fail(Error(`Encoder staging exited ${code}`)));});
export async function stageEncoder(platform=process.platform,architecture=process.arch){
 const arch=typeof architecture==='number'?architectures[architecture]:architecture;
 if(!['win32','darwin','linux'].includes(platform)||!['x64','arm64','ia32'].includes(arch))throw Error('Unsupported AAC encoder target '+platform+'/'+arch);
 const packageRoot=dirname(require.resolve('ffmpeg-static/package.json')),pkg=JSON.parse(await readFile(join(packageRoot,'package.json'),'utf8'));
 if(pkg.version!=='5.3.0')throw Error('Unexpected ffmpeg-static dependency');
 const destination=join(root,'apps/desktop/audio-encoder',platform+'-'+arch);await mkdir(destination,{recursive:true});
 const binary=join(destination,platform==='win32'?'ffmpeg.exe':'ffmpeg');
 await execute(process.execPath,[join(packageRoot,'install.js')],{...process.env,FFMPEG_BIN:binary,npm_config_platform:platform,npm_config_arch:arch,FFMPEG_BINARY_RELEASE:'b6.1.1'});
 // Include the binary build's exact license and source/download instructions.
 await access(binary+'.LICENSE');await access(binary+'.README');await chmod(binary,0o755);
 await writeFile(join(destination,'encoder.json'),JSON.stringify({package:'ffmpeg-static',version:pkg.version,release:'b6.1.1',platform,arch},null,2)+'\n');
 if(platform===process.platform&&arch===process.arch)await execute(binary,['-hide_banner','-version']);
 const licenses=join(root,'apps/desktop/licenses');await mkdir(licenses,{recursive:true});
 await copyFile(join(root,'vendor/faad2/COPYING'),join(licenses,'FAAD2-COPYING.txt'));await copyFile(join(root,'vendor/faad2/SDA-NOTICE.txt'),join(licenses,'FAAD2-NOTICE.txt'));
 return binary;
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url))await stageEncoder();
