import {useEffect, useRef, useState} from "react";
export interface RemoteCommand { id?:string; action:string; value?:unknown }
export interface RemoteTools {
  generator?:{available:boolean;running:boolean;current:number;total:number};
  layout:string;head:string;locked:boolean;dense?:boolean;calibrated?:boolean;error?:string;
  cinema:{profileId:string|null;settings:import("./vite-env").CinemaSettings};
  speakers:{name:string;label:string;az:number;el:number}[];rooms:{id:string;name:string;layout:string;builtin?:boolean}[];heads:{id:string;name:string}[];
}
export interface RemoteScene {objects:import("@sda/player").VisualObject[];layout:readonly import("@sda/renderer").VirtualSpeaker[];muted:number[];sounding:number[];hiddenSpeakers:string[];position:number;trackId:string}
export interface RemotePlayback {
  scene?:RemoteScene;
  coverUrl?:string;
  source?:{codec:string;sampleRate:number;channels:number;objects?:number};
  artist?:string; album?:string;
  tools?:RemoteTools|null;
  loading?:boolean;
  title:string; playing:boolean; paused:boolean; position:number; duration:number; volume:number;
  currentId:string; playbackMode:string; stereoMode:string; playlist:{id:string;title:string}[];
}
export interface RemoteStatus {
  capacity?:number;connectedDevices?:{id:string;name:string;canControl:boolean}[];devices?:{id:string;name:string;canControl:boolean;createdAt:number}[];pendingDevices?:{id:string;name:string;address:string;expires:number}[];
  localMuted?:boolean;
  hlsAllowed?:boolean;
  role:"off"|"host"|"client"; phase:string; detail:string; peer:string|null;
  port:number|null; addresses:string[]; invites:string[]; webInvites?:string[]; format:string;
  bufferMs:number; queuedMs:number; bytes:number;
  output:{actualName?:string;mode?:string;sampleRate?:number;sampleFormat?:string;detail?:string}|null;
  state:RemotePlayback|null;
}
export const EMPTY_REMOTE:RemoteStatus={role:"off",phase:"off",detail:"未连接",peer:null,port:null,
  addresses:[],invites:[],format:"48 kHz · 32-bit float PCM · 双声道",bufferMs:300,queuedMs:0,bytes:0,output:null,state:null};
export function useRemoteSession(state:RemotePlayback,control:(command:RemoteCommand)=>void|Promise<void>,suspend:(replaceOutput:boolean)=>void|Promise<void>,readPlayback?:()=>{loading:boolean;position:number}) {
  const [remote,setRemote]=useState(EMPTY_REMOTE);
  const cover=useRef({source:"",data:""});
  useEffect(()=>{
    const source=state.coverUrl??"";cover.current={source,data:""};
    if(!source)return;let alive=true;const image=new Image();
    image.onload=()=>{
      if(!alive||!image.naturalWidth||!image.naturalHeight)return;
      const scale=Math.min(1,384/Math.max(image.naturalWidth,image.naturalHeight));const canvas=document.createElement("canvas");
      canvas.width=Math.max(1,Math.round(image.naturalWidth*scale));canvas.height=Math.max(1,Math.round(image.naturalHeight*scale));
      try{canvas.getContext("2d")!.drawImage(image,0,0,canvas.width,canvas.height);const data=canvas.toDataURL("image/jpeg",.82);if(data.length<180000)cover.current={source,data};}catch{}
    };image.src=source;return()=>{alive=false;image.onload=null;};
  },[state.coverUrl]);
  const current=useRef({state,control,suspend,readPlayback});current.current={state,control,suspend,readPlayback};
  useEffect(()=>{
    const api=window.sdaDesktop;if(!api?.getRemoteStatus)return;
    let alive=true;
    api.getRemoteStatus().then(value=>{if(alive)setRemote(value);}).catch(()=>{});
    const status=api.onRemoteStatus?.(value=>{if(alive)setRemote(value);});
    let controlQueue=Promise.resolve();
    const commands=api.onRemoteControl?.(command=>{
      if(command.action==="roomCancel"){
        Promise.resolve().then(()=>current.current.control(command)).then(()=>api.completeRemoteControl?.(command.id!,null),error=>api.completeRemoteControl?.(command.id!,String(error)));return;
      }
      controlQueue=controlQueue.catch(()=>{}).then(()=>{}).then(()=>current.current.control(command)).then(
        ()=>api.completeRemoteControl?.(command.id!,null),
        error=>api.completeRemoteControl?.(command.id!,String(error)),
      );
    });
    const pause=api.onRemoteSuspend?.(value=>{void Promise.resolve(current.current.suspend(!!value?.replaceOutput)).catch(console.warn);});
    return()=>{alive=false;status?.();commands?.();pause?.();};
  },[]);
  useEffect(()=>{
    if(remote.role!=="host")return;
    const publish=()=>{const {coverUrl,scene,...state}=current.current.state;window.sdaDesktop?.publishRemoteState?.({...state,...current.current.readPlayback?.(),artwork:cover.current.source===(coverUrl??"")?cover.current.data:""} as RemotePlayback);};
    publish();const timer=setInterval(publish,250);return()=>clearInterval(timer);
  },[remote.role]);
  useEffect(()=>{
    if(remote.role!=="host")return;
    const publish=()=>window.sdaDesktop?.publishRemoteScene?.(current.current.state.scene);
    publish();const timer=setInterval(publish,100);return()=>clearInterval(timer);
  },[remote.role]);
  return remote;
}
