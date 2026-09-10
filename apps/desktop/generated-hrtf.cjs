const fs=require("node:fs"),path=require("node:path"),crypto=require("node:crypto");
async function generate(parameters,assessment,store){
 const {validParameters,parameterKey,synthesizeHrir}=await import("./parametric-hrtf.mjs");
 if(!validParameters(parameters))throw new Error("无效 pHRTF 参数");
 const record=JSON.stringify(assessment??null);
 if(record.length>1024*1024)throw new Error("测试记录过大");
 const digest=crypto.createHash("sha256").update("sda-parametric-hrtf-v1\0"+parameterKey(parameters)+record).digest("hex");
 const id=`personal-${digest}`,target=path.join(store,`hrtf-${id}`);
 fs.mkdirSync(store,{recursive:true});
 if(fs.existsSync(path.join(target,"hrtf-set.json")))return {id,name:"个人生成 pHRTF",method:"parametric-feedback"};
 const staging=fs.mkdtempSync(path.join(store,"generated-"));
 try{
  const positions=[];
  for(const el of [-45,-30,0,30,45,60,90])for(let az=-180;az<180;az+=5){
   if(el===90&&az!==0)continue;
   const name=`az${az<0?"m":""}${Math.abs(az)}_el${el<0?"m":""}${Math.abs(el)}_dry.f32`;
   const packed=synthesizeHrir(az,el,parameters);fs.writeFileSync(path.join(staging,name),Buffer.from(packed.buffer));
   positions.push({azimuth:az,elevation:el,dry:name,wet:name});
  }
  if(parameters.version===2)for(const anchor of parameters.anchors){
   if(positions.some(p=>Math.abs(p.azimuth-anchor.az)<1e-8&&Math.abs(p.elevation-anchor.el)<1e-8))continue;
   const name=`az${positions.length}_el0_dry.f32`,packed=synthesizeHrir(anchor.az,anchor.el,parameters);
   fs.writeFileSync(path.join(staging,name),Buffer.from(packed.buffer));
   positions.push({azimuth:anchor.az,elevation:anchor.el,dry:name,wet:name});
  }
  const manifest={schemaVersion:2,parametricHrtfVersion:1,calibrationVersion:0,sampleRate:48000,completeSubject:true,subjectId:id,
   source:{name:"个人生成 pHRTF",method:"parametric-feedback",parameters,measured:false,description:"Structural approximation selected by subjective responses; not anatomical measurement"},
   processing:{calibrated:false,preserveSamples:true,roomResponse:false,normalization:false},
   azimuthConvention:"positive left; elevation positive up; degrees",positions};
  fs.writeFileSync(path.join(staging,"hrtf-set.json"),JSON.stringify(manifest,null,2));
  fs.writeFileSync(path.join(staging,"assessment.json"),record);
  fs.renameSync(staging,target);
 }finally{if(fs.existsSync(staging))fs.rmSync(staging,{recursive:true,force:true});}
 return {id,name:"个人生成 pHRTF",method:"parametric-feedback"};
}
module.exports={generate};
