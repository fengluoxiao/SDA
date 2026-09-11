// Warm the actual native media element, not a separate AudioContext. Nothing
// from the program is audible until the common start deadline is scheduled.
export function calibrateStartup(owner,onChange=()=>{}){
  const audio=owner.audio,epoch=owner.mediaEpoch,revision=owner.syncRevision;
  if(owner.startupEpoch===epoch)return true;
  if(owner.startupProbe)return false;
  // Match the pause before the scheduled start. An immediate second play
  // measures a hot decoder, which Safari may park during the real 1.2s wait.
  if(owner.startupWarmEpoch===epoch&&performance.now()<(owner.startupWarmReadyAt??0))return false;
  const probe={cancel:null};owner.startupProbe=probe;
  const muted=audio.muted,start=audio.currentTime,began=performance.now();
  let timer,finished=false;
  const valid=()=>!owner.closed&&owner.mediaEpoch===epoch&&owner.syncRevision===revision&&owner.syncPhase==='preparing';
  const finish=(success)=>{
    if(finished)return;finished=true;clearTimeout(timer);
    audio.removeEventListener('timeupdate',progress);audio.removeEventListener('error',failed);
    const elapsed=performance.now()-began,advanced=audio.currentTime-start;
    audio.pause();audio.muted=muted;owner.startupProbe=null;
    if(success&&valid()){
      // First run warms the decoder. Measure a second, warm start because
      // applying cold-start latency to a warm decoder can start it too early.
      if(owner.startupWarmEpoch===epoch){
        owner.startupLeadMs=Math.min(1800,Math.max(0,elapsed-advanced*1000));
        owner.startupEpoch=epoch;
      }else {owner.startupWarmEpoch=epoch;owner.startupWarmReadyAt=performance.now()+1200;}
      audio.currentTime=start;
    }else if(valid()){owner.syncPhase='blocked';owner.syncWaiting=false;}
    onChange();
  };
  const progress=()=>{if(!valid())finish(false);else if(audio.currentTime-start>=.04)finish(true);};
  const failed=()=>finish(false);probe.cancel=()=>finish(false);
  audio.muted=true;
  audio.addEventListener('timeupdate',progress);audio.addEventListener('error',failed);
  timer=setTimeout(failed,5000);
  try{Promise.resolve(audio.play()).then(progress,failed);}catch{failed();}
  return false;
}
