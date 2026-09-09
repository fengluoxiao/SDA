const crypto = require('node:crypto');
const monitorSettings = require('./monitor-settings.cjs');

const names = ['FrontLeft', 'FrontRight', 'Center', 'LFE', 'WideLeft', 'WideRight', 'SurroundLeft', 'SurroundRight', 'RearLeft', 'RearRight', 'TopFrontLeft', 'TopFrontRight', 'TopMiddleLeft', 'TopMiddleRight', 'TopRearLeft', 'TopRearRight'];
const layouts = ['2.0','2.1','5.1','5.1.2','5.1.4','7.1.2','7.1.4','9.1.2','9.1.4','9.1.6'];
const finite = (value, min, max) => typeof value === 'number' && Number.isFinite(value) && value >= min && value <= max;
const text = (value, max = 2048) => typeof value === 'string' && value.trim().length > 0 && value.length <= max;
const reject = message => { throw new Error(message); };

function validateSimulation(value, speakers, layout) {
  const triple=v=>Array.isArray(v)&&v.length===3&&v.every(n=>finite(n,-100,100));
  if(!value||!text(value.engine,128)||!triple(value.size)||!value.size.every(n=>n>0)||!triple(value.listener)
    ||value.config?.layout!==layout||!value.positions||!value.paths||!value.comparison)reject('仿真来源记录无效');
  for(const s of speakers){
    if(!triple(value.positions[s.name])||!Array.isArray(value.paths[s.name])||value.paths[s.name].length!==7)reject('仿真声路无效');
    for(const p of value.paths[s.name]){
      if(!['direct','front','back','left','right','floor','ceiling'].includes(p.wall)||![0,1].includes(p.order)
        ||!finite(p.distance,0,100)||!finite(p.arrivalMs,0,1000)||!Array.isArray(p.points)||p.points.length!==p.order+2||!p.points.every(triple))reject('仿真声路无效');
    }
  }
  for(const mode of ['raw','calibrated','room',...(value.revision>=2?['direct','early']:[])])if(!finite(value.comparison.gainDb?.[mode],-40,0)||!finite(value.comparison.energyDb?.[mode],-300,300))reject('参考电平数据无效');
  if(JSON.stringify(value).length>100000)reject('仿真来源记录过大');
  if(value.revision>=4){
    const r=value.reference,m=value.material;
    if(r?.kind!=='relative-digital'||r.propagationReferenceMetres!==1||r.absoluteSplCalibrated!==false||r.makeupGainDb!==0||r.rirHighpassEnabled!==false)
      reject('房间参考条件无效');
    if(!m||!text(m.id,128)||m.id!==value.config.material||!text(m.source)||!text(m.reference)||!text(m.coverage)
      ||!/^[a-f0-9]{64}$/.test(m.sourceSha256)||!Array.isArray(m.coeffs)||!Array.isArray(m.centerFreqs)
      ||m.coeffs.length!==7||m.centerFreqs.length!==7||!m.coeffs.every(v=>finite(v,0,1))
      ||m.centerFreqs.some((f,i)=>f!==[125,250,500,1000,2000,4000,8000][i]))reject('房间材料来源无效');
  }
  if(value.revision>=5){
    const walls=['east','west','north','south','ceiling','floor'],d=value.studioDesign;
    if(value.config.material!=='studio'||!finite(value.config.listeningDistance,.8,2.5)
      ||!value.surfaces||Object.keys(value.surfaces).length!==6)reject('控制室表面数据无效');
    for(const wall of walls){
      const s=value.surfaces[wall];
      if(!s||!text(s.materialId,128)||!text(s.remainder,128)||!finite(s.coverage,0,1)
        ||!Array.isArray(s.coeffs)||s.coeffs.length!==7||!s.coeffs.every(v=>finite(v,0,1)))reject('控制室表面数据无效');
    }
    if(!d||!text(d.source)||!finite(d.nominalTargetSeconds,.01,10)
      ||d.nearFieldDistanceMetres!==value.config.listeningDistance||!Array.isArray(d.eyringSeconds)
      ||d.eyringSeconds.length!==7||!d.eyringSeconds.every(v=>finite(v,.001,100))
      ||!Array.isArray(d.firstOrderEarlyReflections)||!d.firstOrderEarlyReflections.every(p=>
        speakers.some(s=>s.name===p.speaker)&&['front','back','left','right','floor','ceiling'].includes(p.wall)
        &&finite(p.delayMs,0,15)&&finite(p.worstDb,-300,0)))reject('控制室设计数据无效');
    for(const s of speakers){
      const distance=Math.hypot(...value.positions[s.name].map((v,i)=>v-value.listener[i]));
      if(Math.abs(distance-d.nearFieldDistanceMetres)>1e-6)reject('控制室监听距离不一致');
    }
  }
  return value;
}

function validateSettings(value) {
  if(value?.reflectionMode!==undefined&&!['direct','early','full'].includes(value.reflectionMode))reject('反射试听模式无效');
  if (!value || typeof value.enabled !== 'boolean' || typeof value.bassEnabled !== 'boolean'
    || !finite(value.directDb,-24,6) || !finite(value.earlyDb,-40,6) || !finite(value.lateDb,-40,6)
    || !finite(value.earlyMs,10,100) || !finite(value.crossoverHz,40,160) || !finite(value.bassDb,-24,6)
    || !value.speakers || typeof value.speakers !== 'object' || Array.isArray(value.speakers)) reject('影院设置格式不正确');
  const speakers = {};
  for (const [name, entry] of Object.entries(value.speakers)) {
    if (!names.includes(name) || !entry || !finite(entry.gainDb,-24,6) || !finite(entry.delayMs,0,20)
      || !finite(entry.lowDb,-6,6) || !finite(entry.highDb,-6,6)) reject(`音箱校准值无效：${name}`);
    if (name === 'LFE' && (entry.lowDb !== 0 || entry.highDb !== 0)) reject('LFE 校准仅支持电平与延时');
    speakers[name] = { gainDb:entry.gainDb, delayMs:entry.delayMs, lowDb:entry.lowDb, highDb:entry.highDb };
  }
  const monitor = monitorSettings.validate(value.monitor ?? {...monitorSettings.defaults(),
    enabled:value.enabled && value.bassEnabled,bassEnabled:value.bassEnabled,crossoverHz:value.crossoverHz,bassDb:value.bassDb});
  return { monitor, ...(value.reflectionMode!==undefined?{reflectionMode:value.reflectionMode}:{}), enabled:value.enabled, directDb:value.directDb, earlyDb:value.earlyDb, lateDb:value.lateDb,
    earlyMs:value.earlyMs, bassEnabled:false, crossoverHz:value.crossoverHz, bassDb:value.bassDb, speakers };
}

function validateRoom(value) {
  if (!value || value.version !== 1 || value.sampleRate !== 48000 || !layouts.includes(value.layout)
    || !text(value.name,128) || !text(value.source) || !text(value.license) || !['personal','dummy-head','simulated'].includes(value.measurement)
    || !Array.isArray(value.speakers) || value.speakers.length < 2 || value.speakers.length > 15) reject('房间档案版本、来源或采样率不正确');
  const expected = ['FrontLeft','FrontRight'];
  const floor = Number(value.layout.split('.')[0]);
  const top = Number(value.layout.split('.')[2] || 0);
  if (floor >= 5) expected.push('Center','SurroundLeft','SurroundRight');
  if (floor >= 7) expected.push('RearLeft','RearRight');
  if (floor >= 9) expected.push('WideLeft','WideRight');
  if (top === 2) expected.push('TopMiddleLeft','TopMiddleRight');
  if (top >= 4) expected.push('TopFrontLeft','TopFrontRight','TopRearLeft','TopRearRight');
  if (top === 6) expected.push('TopMiddleLeft','TopMiddleRight');
  if (value.speakers.length !== expected.length || expected.some(name => value.speakers.filter(s => s.name === name).length !== 1)) reject('档案必须覆盖该布局全部非 LFE 音箱');
  const speakers = value.speakers.map(s => {
    if (!finite(s.azimuth,-180,180) || !finite(s.elevation,-90,90) || !Number.isInteger(s.onsetSample)) reject('测量方向或起点无效');
    const angles = {FrontLeft:[30,0],FrontRight:[-30,0],Center:[0,0],WideLeft:[60,0],WideRight:[-60,0],
      SurroundLeft:[floor===5?110:100,0],SurroundRight:[floor===5?-110:-100,0],RearLeft:[140,0],RearRight:[-140,0],
      TopFrontLeft:[45,45],TopFrontRight:[-45,45],TopMiddleLeft:[90,45],TopMiddleRight:[-90,45],TopRearLeft:[135,45],TopRearRight:[-135,45]};
    if (Math.abs(s.azimuth-angles[s.name][0])>1 || Math.abs(s.elevation-angles[s.name][1])>1) reject(`测量方向与布局不一致：${s.name}`);
    const length = s.roomLeft?.length;
    if (!Number.isInteger(length) || length < 512 || length > 32768 || s.onsetSample < 0 || s.onsetSample >= length) reject('响应长度应为 512 至 32768 个采样');
    const result = {name:s.name, azimuth:s.azimuth, elevation:s.elevation, onsetSample:s.onsetSample};
    for (const key of ['directLeft','directRight','roomLeft','roomRight']) {
      const samples = s[key];
      if (!Array.isArray(samples) || samples.length !== length || !samples.every(v => finite(v,-16,16)) || !samples.some(v => Math.abs(v)>1e-9)) reject(`响应数据无效：${s.name}/${key}`);
      result[key] = samples;
    }
    return result;
  });
  const result = {version:1,name:value.name,source:value.source,license:value.license,measurement:value.measurement,sampleRate:48000,layout:value.layout,speakers};
  if(value.measurement==='simulated') {
    result.simulation=validateSimulation(value.simulation,speakers,value.layout);
  }
  return result;
}

function analyzeRoom(profile) {
  const onset = values => {
    const peak = values.reduce((a,b)=>Math.max(a,Math.abs(b)),0);
    return values.findIndex(v=>Math.abs(v)>=peak*0.1);
  };
  const rows = profile.speakers.map(s => {
    const left = onset(s.directLeft), right = onset(s.directRight);
    const power = [...s.directLeft,...s.directRight].reduce((a,b)=>a+b*b,0);
    return { name:s.name, arrivalMs:(left+right)/96, itdMs:(right-left)/48, directEnergyDb:10*Math.log10(power),
      peak:Math.max(...s.roomLeft.map(Math.abs),...s.roomRight.map(Math.abs)) };
  });
  const latest = Math.max(...rows.map(r=>r.arrivalMs));
  const quietest = Math.min(...rows.map(r=>r.directEnergyDb));
  const suggested = {};
  for (const row of rows) suggested[row.name] = {gainDb:Math.max(-24,quietest-row.directEnergyDb),delayMs:Math.min(20,latest-row.arrivalMs),lowDb:0,highDb:0};
  return {rows,suggested,limited:rows.some(r=>latest-r.arrivalMs>20 || quietest-r.directEnergyDb < -24)};
}
function roomSummary(profile, id) {
  return {id,name:profile.name,source:profile.source,license:profile.license,measurement:profile.measurement,layout:profile.layout,sampleRate:profile.sampleRate,...(profile.simulation?{simulation:profile.simulation}:{}),...analyzeRoom(profile)};
}
const roomId = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
module.exports = { validateSettings, validateRoom, analyzeRoom, roomSummary, roomId };
