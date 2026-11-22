import React, { useEffect, useState } from "react";
import { AppState, StyleSheet, type NativeSyntheticEvent } from "react-native";
import { requireNativeView } from "expo";
import { useNavigationPreferences } from "./navigation-preferences";
export { supportsEmbeddedMotionArtwork } from "./motion-artwork-capability";

type Props = {sourceUri:string;visible:boolean;active:boolean;onVisible?(ready:boolean,uri:string):void};
type NativeProps = Omit<Props,"visible"> & {onStatus(event:NativeSyntheticEvent<{uri:string;ready:boolean;unavailable?:boolean}>):void;style:object;pointerEvents:"none"};
const NativeArtwork=(globalThis as any).expo?.modules?.SdaMotionArtwork
 ? (requireNativeView as unknown as <P>(name:string)=>React.ComponentType<P>)<NativeProps>("SdaMotionArtwork") : null;

// Only the large playback artwork mounts this overlay. Static posters, library
// rows and mini-player artwork stay in their existing Image components.
export function EmbeddedMotionArtwork({sourceUri,visible,active,onVisible}:Props) {
 const {motionArtwork}=useNavigationPreferences();
 const [foreground,setForeground]=useState(AppState.currentState==="active");
 useEffect(()=>{const subscription=AppState.addEventListener("change",value=>setForeground(value==="active"));return ()=>subscription.remove();},[]);
 if(!motionArtwork || !NativeArtwork || !visible || !foreground || !sourceUri.startsWith("file:"))return null;
 return <MotionFrame key={sourceUri} sourceUri={sourceUri} active={active} onVisible={onVisible} />;
}
function MotionFrame({sourceUri,active,onVisible}:Omit<Props,"visible">) {
 const [ready,setReady]=useState(false),[failed,setFailed]=useState(false);
 useEffect(()=>()=>onVisible?.(false,sourceUri),[sourceUri,onVisible]);
 if(!NativeArtwork || failed)return null;
 return <NativeArtwork sourceUri={sourceUri} active={active} pointerEvents="none" style={[StyleSheet.absoluteFill,{opacity:ready?1:0}]}
  onStatus={event=>{if(event.nativeEvent.uri!==sourceUri)return;if(event.nativeEvent.unavailable){setFailed(true);onVisible?.(false,sourceUri);}else{setReady(event.nativeEvent.ready);onVisible?.(event.nativeEvent.ready,sourceUri);}}} />;
}
