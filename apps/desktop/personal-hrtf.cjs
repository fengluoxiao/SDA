const fs=require("node:fs");
const path=require("node:path");
const crypto=require("node:crypto");
const {Worker}=require("node:worker_threads");
const PERSONAL_SET=/^hrtf-personal-[a-f0-9]{64}$/;
const readText=(object,name)=>{
  const value=object?.attrs?.[name]?.value;
  return String(Array.isArray(value)?value[0]:value??"").replace(/\0/g,"").trim();
};
function finite(values,name){if(!values||!Array.from(values).every(Number.isFinite))throw new Error(`${name} 包含无效数值`);return Array.from(values);}
function vector(file,name,expected){
  const dataset=file.get(name);if(!dataset)throw new Error(`缺少 ${name}`);
  const type=readText(dataset,"Type").toLowerCase(),units=readText(dataset,"Units").toLowerCase();
  if((type&&type!=="cartesian")||(units&&units!=="metre"))throw new Error(`${name} 需要标准笛卡尔坐标`);
  const v=finite(dataset.value,name);
  if(v.length!==3||v.some((x,i)=>Math.abs(x-expected[i])>1e-6))throw new Error(`${name} 必须使用标准听者坐标，暂不支持移动听者或旋转坐标`);
}
/** Restricted, explicit SOFA convention support. No silent coordinate/rate guesses. */
function decodeSofa(file){
  if(readText(file,"SOFAConventions")!=="SimpleFreeFieldHRIR")throw new Error("目前只支持 SOFA SimpleFreeFieldHRIR（自由场双耳脉冲响应）");
  const ir=file.get("Data.IR"),shape=ir?.shape;
  if(!shape||shape.length!==3||shape[1]!==2||shape[0]<8||shape[0]>4096||shape[2]<16||shape[2]>4096||shape[0]*2*shape[2]>8388608)throw new Error("Data.IR 需要 M×2×N，8–4096 个方向、每耳 16–4096 个采样，总计不超过 8388608 个采样");
  const rate=file.get("Data.SamplingRate");
  if(!rate||readText(rate,"Units").toLowerCase()!=="hertz"||finite(rate.value,"采样率").some(v=>v!==48000))throw new Error("目前只支持 48000 Hz SOFA，请先用专业工具重采样");
  vector(file,"ListenerPosition",[0,0,0]);vector(file,"ListenerView",[1,0,0]);vector(file,"ListenerUp",[0,0,1]);
  const receiver=file.get("ReceiverPosition"),ears=finite(receiver?.value,"ReceiverPosition");
  if(readText(receiver,"Type").toLowerCase()!=="cartesian"||readText(receiver,"Units").toLowerCase()!=="metre"
    ||receiver?.shape?.[0]!==2||receiver?.shape?.[1]!==3||ears.length!==6||!(ears[1]>0&&ears[4]<0))throw new Error("接收器必须是米制笛卡尔坐标，顺序为左耳、右耳");
  const source=file.get("SourcePosition"),positions=finite(source?.value,"SourcePosition");
  if(source?.shape?.length!==2||source.shape[0]!==shape[0]||source.shape[1]!==3)throw new Error("SourcePosition 必须为 M×3");
  const kind=readText(source,"Type").toLowerCase(),units=readText(source,"Units").toLowerCase().replace(/\s/g,"");
  if(!((kind==="spherical"&&units==="degree,degree,metre")||(kind==="cartesian"&&units==="metre")))throw new Error("不支持此 SOFA 声源坐标单位");
  const directions=[];
  for(let m=0;m<shape[0];m++){
    let [a,b,c]=positions.slice(m*3,m*3+3),az,el,distance;
    if(kind==="spherical"){az=((a+180)%360+360)%360-180;el=b;distance=c;}
    else{distance=Math.hypot(a,b,c);az=Math.atan2(b,a)*180/Math.PI;el=Math.atan2(c,Math.hypot(a,b))*180/Math.PI;}
    if(!(distance>0)||el< -90||el>90)throw new Error("声源距离或仰角无效");
    directions.push({azimuth:az,elevation:el,distance});
  }
  if(directions.some(d=>Math.abs(d.distance-directions[0].distance)>.01))throw new Error("目前仅支持单一测量距离，不能把多距离近场数据折叠为一个 HRTF");
  const unique=new Set(directions.map(d=>`${d.azimuth.toFixed(3)},${d.elevation.toFixed(3)}`));
  if(unique.size!==directions.length)throw new Error("SOFA 包含重复方向");
  const delayDataset=file.get("Data.Delay");
  let delays=delayDataset?finite(delayDataset.value,"Data.Delay"):[0,0];
  const delayShape=delayDataset?.shape;
  if(delayDataset&&(!delayShape||delayShape.length!==2||![1,shape[0]].includes(delayShape[0])||delayShape[1]!==2))throw new Error("Data.Delay 必须为 1×2 或 M×2");
  const delayUnits=readText(delayDataset,"Units").toLowerCase();
  if(delayUnits&&!["sample","samples","second","seconds"].includes(delayUnits))throw new Error("不支持此 Data.Delay 单位");
  if(delayUnits.startsWith("second"))delays=delays.map(v=>v*48000);
  if(delays.some(v=>v<0||v>4096))throw new Error("Data.Delay 超出支持范围 0–4096 samples");
  if(shape[0]*2*(shape[2]+Math.ceil(Math.max(...delays))+33)>16777216)throw new Error("应用延迟后的数据超过 64 MB，请先移除不必要的共同传播延迟");
  const samples=finite(ir.value,"Data.IR");
  if(samples.length!==shape[0]*2*shape[2]||samples.some(v=>Math.abs(v)>100)||!samples.some(v=>v!==0))throw new Error("脉冲响应尺寸、幅值异常或全部静音");
  return {directions,delays,samples,frames:shape[2],name:readText(file,"ListenerShortName")||"个人 SOFA",source:readText(file,"DatabaseName")||"用户导入",license:readText(file,"License")};
}
// Windowed-sinc fractional delay, one shared 16-sample processing latency for both ears.
function delayed(input,delay,length){
  const out=new Float32Array(length),integer=Math.floor(delay),fraction=delay-integer;
  if(fraction<1e-9){out.set(input,integer+16);return out;}
  const kernel=[];let sum=0;
  for(let k=0;k<=32;k++){const x=k-16-fraction,sinc=Math.abs(x)<1e-12?1:Math.sin(Math.PI*x)/(Math.PI*x);
    const v=sinc*(.5-.5*Math.cos(2*Math.PI*k/32));kernel.push(v);sum+=v;}
  for(let i=0;i<input.length;i++)for(let k=0;k<=32;k++)if(i+integer+k<length)out[i+integer+k]+=input[i]*kernel[k]/sum;
  return out;
}
async function importSofa(sourcePath,store){
  if(typeof sourcePath!=="string"||!path.isAbsolute(sourcePath)||path.extname(sourcePath).toLowerCase()!==".sofa")throw new Error("请选择 .sofa 文件");
  const stat=fs.statSync(sourcePath);if(!stat.isFile()||stat.size>128*1024*1024)throw new Error("SOFA 文件需小于 128 MB");
  // h5wasm's documented Electron workaround; scoped to this import worker.
  const navigatorDescriptor=Object.getOwnPropertyDescriptor(globalThis,"navigator");
  if(navigatorDescriptor?.configurable)Object.defineProperty(globalThis,"navigator",{value:undefined,configurable:true});
  let h5;
  try{h5=await import("h5wasm/node");await h5.ready;}
  finally{if(navigatorDescriptor?.configurable)Object.defineProperty(globalThis,"navigator",navigatorDescriptor);}
  const file=new h5.File(sourcePath,"r");let data;
  try{data=decodeSofa(file);}finally{file.close();}
  const sourceBytes=fs.readFileSync(sourcePath),sourceSha256=crypto.createHash("sha256").update(sourceBytes).digest("hex");
  const digest=crypto.createHash("sha256").update("sda-personal-sofa-v1\0").update(sourceBytes).digest("hex");
  const set=`hrtf-personal-${digest}`,target=path.join(store,set);
  fs.mkdirSync(store,{recursive:true});
  const staging=fs.mkdtempSync(path.join(store,"import-"));
  try{
    const frames=data.frames+Math.ceil(Math.max(...data.delays))+33;
    const positions=data.directions.map((direction,index)=>{
      const packed=new Float32Array(frames*2);
      for(let ear=0;ear<2;ear++){
        const input=data.samples.slice((index*2+ear)*data.frames,(index*2+ear+1)*data.frames);
        const delay=data.delays[data.delays.length===2?ear:index*2+ear];
        packed.set(delayed(input,delay,frames),ear*frames);
      }
      const name=`az${index}_el0_dry.f32`;fs.writeFileSync(path.join(staging,name),Buffer.from(packed.buffer));
      // No measured room supplied: wet equals dry, so the reflection residual is zero.
      return {...direction,dry:name,wet:name};
    });
    const manifest={schemaVersion:2,calibrationVersion:0,personalSofaVersion:1,sampleRate:48000,completeSubject:true,subjectId:`personal-${digest}`,
      source:{name:data.name,database:data.source,license:data.license,fileName:path.basename(sourcePath),sha256:sourceSha256,method:"imported-sofa",convention:"SimpleFreeFieldHRIR"},
      azimuthConvention:"positive left, degrees; elevation positive up",processing:{calibrated:false,preserveMeasurements:true,commonDelaySamples:16,delayApplied:true,normalization:false,roomResponse:false},positions};
    fs.writeFileSync(path.join(staging,"hrtf-set.json"),JSON.stringify(manifest,null,2));
    if(!fs.existsSync(target))fs.renameSync(staging,target);
  }finally{if(fs.existsSync(staging))fs.rmSync(staging,{recursive:true,force:true});}
  return {id:`personal-${digest}`,name:data.name,directions:data.directions.length,method:"imported-sofa"};
}
function listPersonal(store){
  if(!fs.existsSync(store))return [];
  return fs.readdirSync(store).filter(n=>PERSONAL_SET.test(n)).flatMap(n=>{try{
    const m=JSON.parse(fs.readFileSync(path.join(store,n,"hrtf-set.json"),"utf8"));
    return [require("./personal-hrtf-library.cjs").info(store,n.slice(5))];
  }catch{return [];}});
}
function importInWorker(sourcePath,store,generated){return new Promise((resolve,reject)=>{
  const worker=new Worker(path.join(__dirname,"personal-hrtf-worker.cjs"),{workerData:generated?{kind:"generate",...generated,store}:{sourcePath,store}});
  const timer=setTimeout(()=>{void worker.terminate();reject(new Error("SOFA 导入超时"));},120000);
  worker.once("message",result=>{clearTimeout(timer);result.error?reject(new Error(result.error)):resolve(result.value);});
  worker.once("error",error=>{clearTimeout(timer);reject(error);});
  worker.once("exit",code=>{clearTimeout(timer);if(code)reject(new Error(`SOFA 导入进程退出 (${code})`));});
});}
module.exports={PERSONAL_SET,decodeSofa,delayed,importSofa,importInWorker,listPersonal};
