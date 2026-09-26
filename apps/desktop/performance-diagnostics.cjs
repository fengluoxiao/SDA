// Export only numeric structural facts and a small codec identifier. In particular,
// never forward buffers, arbitrary error strings, decoder memory or file names.
function sanitizeDiagnostic(value){
 if(value?.schema!==1||value.type!=='decoderDiagnostic'||!Array.isArray(value.events))return null;
 const events=[];
 for(const item of value.events.slice(-96)){
  if(!['open','packet','frame','flush','failure','checkpoint'].includes(item?.step))continue;
  const event={step:item.step};
  for(const key of ['sequence','time','bytes','units','sample','samples','rate','channels','objects','epoch','bit','declared','used','code','checkpointDropped'])if(Number.isFinite(item[key]))event[key]=item[key];
  if(typeof item.codec==='string'&&/^[a-z0-9_-]{1,24}$/i.test(item.codec))event.codec=item.codec;
  if(typeof item.checkpoint==='string'&&/^[a-z0-9_.]{1,80}$/.test(item.checkpoint))event.checkpoint=item.checkpoint;
  if(typeof item.errorTag==='string'&&/^[a-f0-9]{16}$/.test(item.errorTag))event.errorTag=item.errorTag;
  for(const key of ['coreHash','framingHash'])if(typeof item[key]==='string'&&/^[a-f0-9]{64}$/.test(item[key]))event[key]=item[key];
  events.push(event);
 }
 return {schema:1,type:'decoderDiagnostic',coverage:events.some(e=>e.checkpoint)?'decoder-checkpoints-v1':'decoder-boundary-only',containsAudio:false,dropped:Math.max(0,Number(value.dropped)||0),events,reproduction:'not-yet-reproduced',missing:'matching generated regression input required; checkpoints alone do not prove reproduction'};
}
module.exports={sanitizeDiagnostic};
