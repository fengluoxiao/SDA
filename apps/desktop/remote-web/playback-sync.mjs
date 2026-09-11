export function syncHostPlayback(owner,state,resume,onError){
  if(!owner?.audio||owner.closed)return;
  const running=!!state.playing&&!state.paused;
  const changed=owner.hostRunning!==running;owner.hostRunning=running;
  if(owner.testing)return;
  if(!running){owner.audio.pause();return;}
  // A browser interruption still needs the explicit resume button. Do not
  // repeatedly call play() for every progress update or undo a local pause.
  if(changed)void resume(owner).catch(onError);
}
