// Carry real decoded PCM into the system media element. No silent companion
// track and no parallel connection to context.destination (which doubles audio).
export function createPcmMediaOutput(context,onChange,createAudio=()=>document.createElement('audio')){
  const destination=context.createMediaStreamDestination(),audio=createAudio();
  destination.channelCount=2;destination.channelCountMode='explicit';
  audio.hidden=true;audio.autoplay=false;audio.setAttribute('playsinline','');
  audio.srcObject=destination.stream;document.body.append(audio);
  let closed=false,pending=null;
  for(const name of ['playing','pause','waiting','error'])audio.addEventListener(name,onChange);
  return {audio,destination,
    play(){
      if(closed)return Promise.reject(Error('PCM 输出已关闭'));
      if(pending)return pending;
      pending=Promise.resolve(audio.play()).finally(()=>{pending=null;});return pending;
    },
    pause(){audio.pause();},
    close(){
      closed=true;for(const name of ['playing','pause','waiting','error'])audio.removeEventListener(name,onChange);
      audio.pause();audio.srcObject=null;destination.disconnect();
      for(const track of destination.stream.getTracks())track.stop();audio.remove();
    },
  };
}
