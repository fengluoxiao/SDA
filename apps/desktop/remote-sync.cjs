"use strict";
// One playout clock for the native DAC and every HLS receiver. No UI-only delay.
class RemoteSync {
  constructor(hooks={}){this.hooks=hooks;this.peers=new Set();this.revision=0;this.phase="idle";this.state={};this.settledAt=0;}
  bufferMs(){
    // Requesting parts does not prove Safari uses a subsecond seekable edge.
    // Keep startup's three full segments plus margin, and account for the
    // measured gap between its buffered end and seekable end before shrinking.
    if(!this.peers.size||![...this.peers].every(p=>p.lowLatency&&Number.isFinite(p.nativeHoldbackSeconds)))return 6000;
    return Math.min(6000,Math.max(4000,...[...this.peers].map(p=>Math.max((p.clockRtt??0)*4+1000,p.nativeHoldbackSeconds*1000+1500))));
  }
  add(peer){this.peers.add(peer);this.hold("join");}
  remove(peer){this.peers.delete(peer);if(!this.peers.size){this.revision++;this.phase="idle";void this.hooks.gate?.({enabled:false});}else this.hold("leave");}
  log(reason,extra={}){this.hooks.diagnostic?.({sync:true,phase:this.phase,revision:this.revision,reason,...extra});}
  hold(reason){
    if(!this.peers.size)return;
    // Repeated taps while loading must not keep invalidating the same barrier.
    if(reason==='browser-play-request'&&['holding','preparing','arming'].includes(this.phase))return;
    const at=this.phase==='running'?Date.now()+150:0;
    const revision=++this.revision;this.phase="holding";this.settledAt=(at||Date.now())+350;
    for(const peer of this.peers){peer.syncReady=false;peer.forward("Y",{action:"hold",revision,at,waiting:!!this.state.playing&&!this.state.paused});}
    void Promise.resolve(this.hooks.gate?.({enabled:true,bufferMs:this.bufferMs(),startAtMs:at?1:0,stopAtMs:at})).then(accepted=>{if(this.revision===revision&&this.phase==='holding')this.phase=accepted===false?'error':'preparing';}).catch(()=>{if(this.revision===revision){this.phase="error";this.log("native-gate-failed");}});
    this.log(reason);
  }
  update(state){
    const wasRunning=this.state.playing&&!this.state.paused;this.state=state;
    const running=state.playing&&!state.paused;
    if(!running&&wasRunning&&this.peers.size){this.hold("pause-or-end");}
    if(running&&!wasRunning&&this.peers.size)this.hold("play");
  }
  reset(){this.hold("media-reset");}
  feedback(peer,r){
    if(!this.peers.has(peer)||r.epoch!==peer.epoch)return;
    const buffered=r.ranges?.buffered,seekable=r.ranges?.seekable;
    const bufferedEnd=buffered?.at(-1)?.[1],seekableEnd=seekable?.at(-1)?.[1];
    if(Number.isFinite(bufferedEnd)&&Number.isFinite(seekableEnd)&&seekableEnd>0){
      const gap=Math.max(0,bufferedEnd-seekableEnd);
      // A high-water estimate prevents oscillation as partial segments arrive.
      peer.nativeHoldbackSeconds=Math.max(peer.nativeHoldbackSeconds??0,Math.min(10,gap));
    }
    if(Number.isFinite(r.clockRtt)&&r.clockRtt>=0&&r.clockRtt<=3000)peer.clockRtt=r.clockRtt;
    if(this.phase==="running"){
      const now=Date.now(),running=this.state.playing&&!this.state.paused;
      if(running&&r.syncPhase==='blocked'){this.hold('browser-play-blocked');this.phase='blocked';return;}
      if(running&&r.syncPhase==='holding'&&now-(this.startedAt??0)>100){this.hold('start-not-confirmed');return;}
      const nearEnd=Number.isSafeInteger(peer.endSample)&&(peer.positionBase??0)+r.time>=peer.endSample/48000-.4;
      if(running&&!nearEnd&&!r.pending&&!r.paused&&(r.waiting&&r.readyState<3||r.aheadMs<250)&&now-(this.startedAt??0)>1500){this.hold("receiver-buffer-low");return;}
      // A browser can start late or change its playback clock after a stall.
      // Realign consumers at a held native sample, never move only the UI.
      if(running&&!r.paused&&r.syncPhase==='running'&&r.revision===this.revision&&now-(this.startedAt??0)>2000&&Number.isFinite(r.hostTime)&&Math.abs(now-r.hostTime)<2000){
        const native=this.hooks.position?.();
        const remote=(peer.positionBase??0)+r.time+(now-r.hostTime)/1000;
        if(Number.isFinite(native)&&Math.abs(native-remote)>.3){
          peer.driftCount=(peer.driftCount??0)+1;
          if(peer.driftCount>=3){
            peer.driftCount=0;
            // Calling HTMLMediaElement.play() is not the moment Safari's media
            // clock starts. Learn its startup delay instead of repeating the
            // identical late start on every resynchronization.
            const skewMs=1000*(native-remote);
            if(now-this.startedAt<10000&&Math.abs(skewMs)<2000){
              peer.playLeadMs=Math.min(1800,Math.max(0,(peer.playLeadMs??0)+skewMs));
              peer.measuredPlaybackLead=true;
            }
            this.log('measured-clock-drift',{skewMs:Math.round(skewMs),playLeadMs:Math.round(peer.playLeadMs??0)});
            this.hold("clock-drift");return;
          }
        }else peer.driftCount=0;
      }
      return;
    }
    if(this.phase!=="preparing"||Date.now()<this.settledAt||!this.state.playing||this.state.paused)return;
    const position=this.hooks.position?.();if(!Number.isFinite(position))return;
    if(peer.preparedRevision!==this.revision){
      peer.preparedRevision=this.revision;peer.syncReady=false;
      peer.forward("Y",{action:"prepare",revision:this.revision,position,minBufferSeconds:Math.min(3,Math.max(1,(peer.clockRtt??0)/1000*4))});return;
    }
    if(r.revision!==this.revision||r.syncReady!==true)return;
    if(Number.isFinite(r.startupLeadMs)&&r.startupLeadMs>=0&&r.startupLeadMs<=1800&&peer.calibratedEpoch!==peer.epoch){
      if(!peer.measuredPlaybackLead)peer.playLeadMs=r.startupLeadMs;
      peer.calibratedEpoch=peer.epoch;
      this.log('startup-calibrated',{playLeadMs:Math.round(peer.playLeadMs)});
    }
    peer.syncReady=true;
    if(this.state.playing&&!this.state.paused&&[...this.peers].every(p=>p.syncReady))void this.start();
  }
  async start(){
    if(this.phase!=="preparing")return;
    this.phase="arming";const revision=this.revision;
    // Allow the start instruction to cross a slow tunnel before its deadline.
    const lead=Math.min(4500,Math.max(1200,...[...this.peers].map(p=>(p.clockRtt??0)*2+600+(p.playLeadMs??0))));
    const at=Date.now()+lead;
    try{
      const accepted=await this.hooks.gate?.({enabled:true,bufferMs:this.bufferMs(),startAtMs:at});
      if(this.revision!==revision)return;
      if(accepted===false||Date.now()>at-400){this.hold("late-start-arm");return;}
      for(const peer of this.peers){peer.driftCount=0;peer.forward("Y",{action:"start",revision,at,playLeadMs:peer.playLeadMs??0});}
      this.phase="running";this.startedAt=at;this.log("scheduled-start",{at});
    }catch{if(this.revision===revision)this.hold("start-failed");}
  }
}
module.exports={RemoteSync};
