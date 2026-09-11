// A control click must silence cached audio before its network round trip.
export function beginTrackSwitch(owner,awaitEpoch=true){
  if(!owner||owner.closed)return;
  owner.pendingStart=true;owner.mediaPending=true;owner.switchingTrack=true;
  owner.awaitingMediaEpoch=awaitEpoch?(owner.mediaEpoch??-1):undefined;owner.displayPosition=0;owner.mediaClockBase=0;
  owner.hostRunning=false;owner.mediaWaiting=true;
  if(owner.synchronized){clearTimeout(owner.syncTimer);owner.syncPhase='holding';owner.syncWaiting=true;}
  owner.startupProbe?.cancel();
  if(owner.audio){owner.audio.pause();owner.audio.removeAttribute("src");owner.audio.load();}
  else owner.node?.port.postMessage({type:"hold",enabled:true});
}
export function acceptHlsMedia(owner,info){
  if(!Number.isSafeInteger(info.mediaEpoch)||info.mediaEpoch<0)return false;
  if(info.mediaEpoch<(owner.mediaEpoch??-1))return false;
  owner.lastMediaInfo=info;
  if(owner.awaitingMediaEpoch!==undefined&&info.mediaEpoch<=owner.awaitingMediaEpoch)return false;
  if(owner.mediaEpoch!==info.mediaEpoch){
    if(owner.startupProbe){owner.syncPhase='holding';owner.startupProbe.cancel();}
    owner.mediaEpoch=info.mediaEpoch;owner.mediaPending=true;owner.mediaWaiting=true;owner.hostRunning=false;
    owner.audio.pause();owner.audio.removeAttribute("src");owner.audio.load();
  }
  if(!info.mediaReady)return false;
  owner.awaitingMediaEpoch=undefined;
  if(owner.loadedMediaEpoch===info.mediaEpoch)return false;
  owner.loadedMediaEpoch=info.mediaEpoch;owner.hlsClock=true;
  owner.mediaClockBase=info.synchronized?Math.max(0,Number(info.positionBase)||0):owner.switchingTrack?0:Math.max(0,Number(info.positionBase)||0);
  owner.switchingTrack=false;owner.mediaPending=false;owner.mediaWaiting=true;
  owner.audio.src=info.stream;owner.audio.load();return true;
}

export function syncHostPlayback(owner,state,resume,onError){
  if(!owner?.audio||owner.closed)return;
  if(owner.synchronized){owner.hostRunning=!!state.playing&&!state.paused;return;}
  const running=!!state.playing&&!state.paused;
  if(state.loading||owner.mediaPending){owner.hostRunning=false;owner.audio.pause();return;}
  const changed=owner.hostRunning!==running;owner.hostRunning=running;
  if(owner.testing)return;
  if(!running){owner.audio.pause();return;}
  // A browser interruption still needs the main play button. Do not
  // repeatedly call play() for every progress update or undo a local pause.
  if(changed)void resume(owner).catch(onError);
}

// HLS time is relative to its media epoch; add that epoch's song offset.
// Hold presentation while the receiver cannot produce audio.
export function receiverView(owner, state) {
  const running=!!state?.playing&&!state.paused;
  const blocked=!!owner&&(owner.audio?owner.audio.paused:owner.context?.state!=="running"||owner.pcmOutput?.audio.paused);
  const loading=!!owner?.pendingStart||!state?.paused&&(!!state?.loading||
    !!owner?.mediaPending&&(running||!!owner?.switchingTrack)||running&&!!owner?.syncWaiting);
  const buffering=!owner?.testing&&(loading||running&&!blocked&&(!owner?.ready||
    (owner.audio?owner.mediaWaiting!==false||owner.audio.readyState<3||owner.audio.seeking:owner.buffering)));
  const audible=running&&!blocked&&!buffering&&!owner?.testing;
  const key=JSON.stringify([state?.currentId,state?.title]);
  const position=Math.max(0,Number(state?.position)||0);
  if(owner){
    if(owner.progressKey!==key){owner.progressKey=key;owner.displayPosition=owner.mediaPending?0:position;}
    if(owner.switchingTrack||owner.mediaPending)owner.displayPosition=0;
    else if(audible||!running)owner.displayPosition=owner.hlsClock
      ?Math.max(0,(Number(owner.audio.currentTime)||0)+(owner.mediaClockBase||0)):position;
  }
  return {audible,buffering,position:owner?.displayPosition??position,
    label:owner?.testing?"耳廓测试中":loading?"正在加载音频…":!running?(state?.paused?"已暂停":"等待播放"):
      blocked?"点击播放收听":buffering?"正在缓冲…":"正在播放"};
}

export function cancelTrackSwitch(owner){
  owner.pendingStart=false;owner.switchingTrack=false;owner.awaitingMediaEpoch=undefined;
  if(owner.audio){
    owner.loadedMediaEpoch=undefined;
    if(owner.lastMediaInfo)acceptHlsMedia(owner,owner.lastMediaInfo);
  }else{owner.mediaPending=false;owner.node?.port.postMessage({type:"hold",enabled:false});}
}
