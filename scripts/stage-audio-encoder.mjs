import {mkdir,readFile,copyFile,chmod,writeFile,access} from 'node:fs/promises';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createRequire} from 'node:module';
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
const require=createRequire(import.meta.url),root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const architectures={0:'ia32',1:'x64',2:'armv7l',3:'arm64'};
const execute=(exe,args,env=process.env)=>new Promise((done,fail)=>{const child=spawn(exe,args,{stdio:'inherit',windowsHide:true,env});child.once('error',fail);child.once('exit',code=>code===0?done():fail(Error(`Encoder staging exited ${code}`)));});
export const windowsArmEncoder={
 release:'autobuild-2026-10-28-19-47',
 asset:'ffmpeg-n8.1.3-25-g611cec792a-winarm64-gpl-8.1.zip',
 sha256:'52882901d205718fac834b80f25aed16ecece0f4ef6c79b2ed2d5061581587a6',
};
async function stageWindowsArmEncoder(){
 const pin=windowsArmEncoder,url=`https://github.com/BtbN/FFmpeg-Builds/releases/download/${pin.release}/${pin.asset}`;
 const response=await fetch(url);if(!response.ok)throw Error('Native ARM64 encoder download failed: '+response.status);
 const bytes=Buffer.from(await response.arrayBuffer());if(createHash('sha256').update(bytes).digest('hex')!==pin.sha256)throw Error('Native ARM64 encoder archive hash mismatch');
 const temp=join(root,'tmp','desktop-encoder-arm64.zip');await mkdir(dirname(temp),{recursive:true});await writeFile(temp,bytes);
 const destination=join(root,'apps/desktop/audio-encoder/win32-arm64');await mkdir(destination,{recursive:true});
 const extract=`import sys,zipfile,struct,pathlib
z=zipfile.ZipFile(sys.argv[1]); out=pathlib.Path(sys.argv[2])
for suffix,target in [('/bin/ffmpeg.exe','ffmpeg.exe'),('/LICENSE.txt','ffmpeg.exe.LICENSE')]:
 names=[n for n in z.namelist() if n.endswith(suffix)]
 assert len(names)==1, suffix
 data=z.read(names[0])
 if target=='ffmpeg.exe':
  assert data[:2]==b'MZ'
  pe=struct.unpack_from('<I',data,0x3c)[0]
  assert data[pe:pe+4]==bytes([80,69,0,0]) and struct.unpack_from('<H',data,pe+4)[0]==0xaa64
 (out/target).write_bytes(data)
`;
 await execute(process.env.PYTHON??'python',['-c',extract,temp,destination]);
 await writeFile(join(destination,'ffmpeg.exe.README'),`Native Windows ARM64 FFmpeg\nUpstream: BtbN/FFmpeg-Builds\nRelease: ${pin.release}\nArchive: ${pin.asset}\nSHA-256: ${pin.sha256}\nFFmpeg source: https://github.com/FFmpeg/FFmpeg/commit/611cec792a\nBuild recipes and dependency source instructions: https://github.com/BtbN/FFmpeg-Builds/tree/${pin.release}\nLicense: see ffmpeg.exe.LICENSE\n`);
 await writeFile(join(destination,'encoder.json'),JSON.stringify({package:'BtbN/FFmpeg-Builds',platform:'win32',arch:'arm64',...pin},null,2)+'\n');
 const licenses=join(root,'apps/desktop/licenses');await mkdir(licenses,{recursive:true});
 await copyFile(join(root,'vendor/faad2/COPYING'),join(licenses,'FAAD2-COPYING.txt'));await copyFile(join(root,'vendor/faad2/SDA-NOTICE.txt'),join(licenses,'FAAD2-NOTICE.txt'));
 const binary=join(destination,'ffmpeg.exe');if(process.platform==='win32'&&process.arch==='arm64')await execute(binary,['-hide_banner','-version']);return binary;
}
export async function stageEncoder(platform=process.platform,architecture=process.arch){
 const arch=typeof architecture==='number'?architectures[architecture]:architecture;
 if(!['win32','darwin','linux'].includes(platform)||!['x64','arm64','ia32'].includes(arch))throw Error('Unsupported AAC encoder target '+platform+'/'+arch);
 if(platform==='win32'&&arch==='arm64')return stageWindowsArmEncoder();
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
