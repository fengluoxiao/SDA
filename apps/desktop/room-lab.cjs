const fs=require('node:fs');
const path=require('node:path');
const crypto=require('node:crypto');
const {spawn}=require('node:child_process');
const profiles=require('./cinema-profiles.cjs');

const layouts=['2.0','2.1','5.1','5.1.2','5.1.4','7.1.2','7.1.4','9.1.2','9.1.4','9.1.6'];
function validateConfig(input) {
  if(!input||!layouts.includes(input.layout)||!['treated','living','reflective'].includes(input.material))throw new Error('仿真布局或材料无效');
  const result={layout:input.layout,material:input.material};
  for(const [key,min,max] of [['length',3,10],['width',3,8],['height',2.2,4],['earHeight',.8,1.6],['placement',.5,1],['order',1,12]]){
    const value=input[key];if(typeof value!=='number'||!Number.isFinite(value)||value<min||value>max)throw new Error(`仿真参数超出范围：${key}`);
    result[key]=value;
  }
  if(!Number.isInteger(result.order))throw new Error('反射阶数必须是整数');
  const [floor,,top=0]=input.layout.split('.').map(Number);
  const list=[['FrontLeft',30,0],['FrontRight',-30,0]];
  if(floor>=5)list.push(['Center',0,0],['SurroundLeft',floor===5?110:100,0],['SurroundRight',floor===5?-110:-100,0]);
  if(floor>=7)list.push(['RearLeft',140,0],['RearRight',-140,0]);
  if(floor>=9)list.push(['WideLeft',60,0],['WideRight',-60,0]);
  if(top===2||top===6)list.push(['TopMiddleLeft',90,45],['TopMiddleRight',-90,45]);
  if(top>=4)list.push(['TopFrontLeft',45,45],['TopFrontRight',-45,45],['TopRearLeft',135,45],['TopRearRight',-135,45]);
  result.speakers=list.map(([name,azimuth,elevation])=>({name,azimuth,elevation}));
  return result;
}

function createRoomLab({runtimeFile,storeRoot,assetsRoot}) {
  let child=null,progress={running:false,current:0,total:0,error:null};
  const runtime=()=>{
    if(!fs.existsSync(runtimeFile))throw new Error('尚未配置房间仿真运行环境');
    const config=JSON.parse(fs.readFileSync(runtimeFile,'utf8'));
    for(const key of ['python','script','source','hrtf'])if(typeof config[key]!=='string'||!fs.existsSync(config[key]))throw new Error(`仿真文件缺失：${key}`);
    return config;
  };
  return {
    status(){try{runtime();return {...progress,available:true};}catch(e){return {...progress,available:false,error:e.message};}},
    cancel(){if(child){progress.error='已取消';child.kill();return true;}return false;},
    async generate(input){
      if(child)throw new Error('已有房间正在生成');
      const config=validateConfig(input),run=runtime();
      const job=path.join(storeRoot,'room-jobs',crypto.randomUUID());
      fs.mkdirSync(job,{recursive:true});
      const configPath=path.join(job,'config.json'),output=path.join(job,'room.json');
      fs.writeFileSync(configPath,JSON.stringify(config));
      progress={running:true,current:0,total:config.speakers.length,error:null};
      try{
        await new Promise((resolve,reject)=>{
          child=spawn(run.python,[run.script,'--config',configPath,'--source',run.source,'--hrtf',run.hrtf,'--assets',assetsRoot,'--output',output],{
            windowsHide:true,env:{...process.env,PYTHONPATH:run.pythonPath??'',OPENBLAS_NUM_THREADS:'1',OMP_NUM_THREADS:'1'},stdio:['ignore','pipe','pipe']});
          let stderr='',pending='';
          const timeout=setTimeout(()=>{progress.error='仿真超过十分钟';child?.kill();},600000);
          child.stdout.on('data',bytes=>{pending+=bytes.toString();let cut;while((cut=pending.indexOf('\n'))>=0){const line=pending.slice(0,cut);pending=pending.slice(cut+1);try{const v=JSON.parse(line);if(Number.isInteger(v.progress))progress.current=Math.min(progress.total,Math.max(0,v.progress));}catch{}}if(pending.length>65536)pending='';});
          child.stderr.on('data',bytes=>{stderr=(stderr+bytes.toString()).slice(-12000);});
          child.once('error',e=>{clearTimeout(timeout);reject(e);});
          child.once('close',code=>{clearTimeout(timeout);code===0?resolve():reject(new Error(progress.error||stderr||`仿真退出 ${code}`));});
        });
        if(fs.statSync(output).size>64*1024*1024)throw new Error('仿真响应超过 64 MB');
        const profile=profiles.validateRoom(JSON.parse(fs.readFileSync(output,'utf8')));
        const bytes=Buffer.from(JSON.stringify(profile)),id=profiles.roomId(bytes);
        const destination=path.join(storeRoot,'cinema-rooms');fs.mkdirSync(destination,{recursive:true});
        fs.writeFileSync(path.join(destination,`${id}.json`),bytes);
        return profiles.roomSummary(profile,id);
      }catch(e){progress.error=e.message;throw e;}
      finally{child=null;progress.running=false;fs.rmSync(job,{recursive:true,force:true});}
    }
  };
}
module.exports={validateConfig,createRoomLab};
