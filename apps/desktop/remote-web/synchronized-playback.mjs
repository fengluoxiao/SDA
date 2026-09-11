export const localClock=()=>performance.timeOrigin+performance.now();
// Native HLS may round the first decoded timestamp above the requested sample.
// Only bridge timestamp rounding (20 ms), never a missing segment or live edge.
const RANGE_TOLERANCE=.02;
export function mediaRanges(audio){
  const read=r=>Array.from({length:Math.min(r?.length??0,8)},(_,i)=>[r.start(i),r.end(i)]);
  return {buffered:read(audio.buffered),seekable:read(audio.seekable),seeking:!!audio.seeking};
}
function rangeTarget(ranges,target,includeEnd=false){
  for(let i=0;i<(ranges?.length??0);i++){
    const start=ranges.start(i),end=ranges.end(i);
    if(target>=start-RANGE_TOLERANCE&&(target<end||includeEnd&&target===end))return Math.max(target,start);
  }
  return null;
}
export function bufferedAhead(audio){
  for(let i=0;i<audio.buffered.length;i++)if(audio.currentTime>=audio.buffered.start(i)-RANGE_TOLERANCE&&audio.currentTime<=audio.buffered.end(i))return audio.buffered.end(i)-Math.max(audio.currentTime,audio.buffered.start(i));
  return 0;
}
// Safari can stop requesting live HLS after a synchronized pause even though
// later segments exist. Reload the same stream, then let preparation seek to
// the held sample. Never play here or release the shared readiness barrier.
export function recoverSynchronizedMedia(owner,now=performance.now()){
  if(!owner.synchronized||owner.syncPhase!=='preparing'||owner.closed||owner.mediaPending||owner.hostRunning===false){owner.syncRecovery=null;return false;}
  const audio=owner.audio;if(!audio)return false;
  const key=`${owner.mediaEpoch}:${owner.syncRevision}:${owner.syncPosition}`;
  const end=audio.buffered.length?audio.buffered.end(audio.buffered.length-1):0;
  let state=owner.syncRecovery;
  if(!state||state.key!==key){owner.syncRecovery={key,end,changedAt:now,attempts:0};return false;}
  if(end>state.end+.02){state.end=end;state.changedAt=now;}
  if(now-state.changedAt<10000||state.attempts>=2||!audio.paused)return false;
  // Full buffers waiting for another receiver do not need a media reload.
  const required=Math.min(owner.syncMinBuffer??1,Math.max(.05,(owner.songDuration??Infinity)-owner.syncPosition));
  if(audio.readyState>=3&&!audio.seeking&&bufferedAhead(audio)>=required)return false;
  state.attempts++;state.changedAt=now;state.end=0;
  audio.load();return true;
}
export function prepareSynchronizedMedia(owner){
  if(owner.startupProbe)return false;
  if(!owner.synchronized||owner.syncPhase!=="preparing"||owner.mediaPending||!owner.audio.readyState||!Number.isFinite(owner.clockOffset))return false;
  const target=Math.max(0,owner.syncPosition-(owner.mediaClockBase??0)),audio=owner.audio;
  if(!Number.isFinite(target))return false;
  if(Math.abs(audio.currentTime-target)>.02&&!audio.seeking){
    // Seeking requests the missing HLS segment. Requiring it to be buffered
    // before seeking deadlocks a paused Safari element after resynchronization.
    // Safari may buffer past its permitted seekable edge. Seeking there is
    // clamped backwards; repeatedly retrying the same target cannot fix it.
    const available=audio.seekable?.length?rangeTarget(audio.seekable,target,true):rangeTarget(audio.buffered,target);
    if(available===null)return false;
    audio.currentTime=available;return false;
  }
  return !audio.seeking&&audio.readyState>=3&&bufferedAhead(audio)>=Math.min(owner.syncMinBuffer??1,Math.max(.05,(owner.songDuration??Infinity)-owner.syncPosition));
}
export function synchronizeMedia(owner,message,onChange=()=>{}){
  if(message.action==='clock'){
    const now=localClock(),rtt=now-message.echo;
    if(!Number.isFinite(rtt)||rtt<0||rtt>3000||!Number.isFinite(message.hostTime))return;
    if(!Number.isFinite(owner.clockRtt)||rtt<owner.clockRtt||now-(owner.clockMeasuredAt??0)>30000){owner.clockRtt=rtt;owner.clockOffset=message.hostTime-(message.echo+now)/2;owner.clockMeasuredAt=now;}
    return;
  }
  if(!Number.isSafeInteger(message.revision)||message.revision<(owner.syncRevision??-1))return;
  owner.syncRevision=message.revision;clearTimeout(owner.syncTimer);
  if(owner.startupProbe){owner.syncPhase='holding';owner.startupProbe.cancel();}
  if(message.action==='hold'||message.action==='prepare'){
    const hold=()=>{owner.syncWaiting=message.waiting!==false;if(message.waiting===false)owner.pendingStart=false;owner.audio.pause();onChange();};
    owner.syncPhase=message.action==='prepare'?'preparing':'holding';
    const delay=Number.isFinite(owner.clockOffset)&&message.at?message.at-(localClock()+owner.clockOffset):0;
    if(delay>0&&delay<2000)owner.syncTimer=setTimeout(hold,delay);else hold();
    if(message.action==='prepare'){
      owner.syncPosition=message.position;
      owner.syncMinBuffer=Number.isFinite(message.minBufferSeconds)?Math.min(3,Math.max(1,message.minBufferSeconds)):1;
    }
    onChange();return;
  }
  if(message.action==='start'&&Number.isFinite(message.at)&&Number.isFinite(owner.clockOffset)){
    const playLeadMs=Number.isFinite(message.playLeadMs)?Math.min(1800,Math.max(0,message.playLeadMs)):0;
    const delay=message.at-playLeadMs-(localClock()+owner.clockOffset);
    owner.syncStartDelay=delay;
    if(delay<0||delay>5000){owner.syncPhase='holding';owner.syncWaiting=true;onChange();return;}
    owner.syncPhase='armed';
    // Clock estimates can improve while armed. They must not retroactively
    // change the deadline used by an already scheduled browser timer.
    const deadline=performance.now()+delay;
    owner.syncTimer=setTimeout(()=>{
      if(owner.closed||owner.syncRevision!==message.revision)return;
      owner.syncTimerLateMs=performance.now()-deadline;
      if(owner.syncTimerLateMs>100){owner.syncPhase='holding';owner.syncWaiting=true;onChange();return;}
      owner.syncPhase='running';owner.syncWaiting=false;
      void owner.audio.play().then(onChange,()=>{owner.syncWaiting=false;owner.syncPhase='blocked';onChange();});
    },delay);
  }
}
