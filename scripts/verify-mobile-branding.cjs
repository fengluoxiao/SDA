// Verify the configured icon survives Expo's actual iOS asset generation.
const assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
const repo=path.resolve(__dirname,'..'),mobile=path.join(repo,'apps/mobile');
const {getPngInfo}=require(require.resolve('@expo/image-utils',{paths:[mobile]}));
const config=JSON.parse(fs.readFileSync(path.join(mobile,'app.json'),'utf8')).expo;
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
const arg=name=>{const i=process.argv.indexOf(name);return i<0?undefined:process.argv[i+1];};
(async()=>{
 const source=path.resolve(mobile,config.ios.icon??config.icon),expected=await getPngInfo(source);
 assert.equal(expected.width,1024);assert.equal(expected.height,1024);
 for(let i=3;i<expected.data.length;i+=4)assert.equal(expected.data[i],255,'iOS icon must be opaque');
 const report={schemaVersion:1,fullName:'Spatial Decoder Atelier',configuredIcon:path.relative(repo,source),sourceSha256:sha(fs.readFileSync(source)),pixelSha256:sha(expected.data),width:1024,height:1024,opaque:true};
 for(const name of ['AndroidSettings.tsx','IOSPlayer.tsx','IOSNativeSettings.tsx']){
  const text=fs.readFileSync(path.join(mobile,'src',name),'utf8');assert(text.includes(report.fullName));
 }
 const folder=arg('--ios-assets');
 if(folder){
  const asset=path.resolve(folder),contents=JSON.parse(fs.readFileSync(path.join(asset,'Contents.json'),'utf8'));
  const item=contents.images.find(i=>i.filename&&!i.appearances);assert(item,'generated universal icon absent');
  const file=path.resolve(asset,item.filename);assert(path.dirname(file)===asset,'invalid icon asset path');
  const actual=await getPngInfo(file);assert.equal(actual.width,1024);assert.equal(actual.height,1024);
  assert.equal(sha(actual.data),report.pixelSha256,'Expo generated an icon different from the configured desktop-derived mark');
  report.generatedIconSha256=sha(fs.readFileSync(file));report.generatedPixelsMatch=true;
 }
 if(arg('--receipt'))fs.writeFileSync(path.resolve(arg('--receipt')),JSON.stringify(report,null,2));
 console.log(JSON.stringify(report));
})().catch(e=>{console.error(e);process.exitCode=1;});
