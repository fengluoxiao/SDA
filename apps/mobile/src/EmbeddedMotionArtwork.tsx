import React, { useCallback, useEffect, useRef, useState } from "react";
import { AppState, StyleSheet, type NativeSyntheticEvent } from "react-native";
import { requireNativeView } from "expo";
import { useNavigationPreferences } from "./navigation-preferences";
export { supportsEmbeddedMotionArtwork } from "./motion-artwork-capability";

type Props = {sourceUri:string;motionKey?:string;visible:boolean;active:boolean;blurred?:boolean;opacity?:number;onVisible?(ready:boolean,uri:string):void};
type NativeProps = Omit<Props,"visible"|"opacity"|"motionKey"> & {onStatus(event:NativeSyntheticEvent<{uri:string;ready:boolean;unavailable?:boolean}>):void;style:object;pointerEvents:"none"};
const NativeArtwork=(globalThis as any).expo?.modules?.SdaMotionArtwork
 ? (requireNativeView as unknown as <P>(name:string)=>React.ComponentType<P>)<NativeProps>("SdaMotionArtwork") : null;

// Only the large playback artwork mounts this overlay. Static posters, library
// rows and mini-player artwork stay in their existing Image components.
export function EmbeddedMotionArtwork({sourceUri,motionKey,visible,active,blurred,opacity,onVisible}:Props) {
 const {motionArtwork}=useNavigationPreferences();
 const [foreground,setForeground]=useState(AppState.currentState==="active");
 useEffect(()=>{const subscription=AppState.addEventListener("change",value=>setForeground(value==="active"));return ()=>subscription.remove();},[]);
 // Keep the native player mounted for tab changes and app backgrounds. The
 // lifecycle/active flags pause it, while its last rendered frame remains.
 if(!motionArtwork || !NativeArtwork || !sourceUri.startsWith("file:"))return null;
 return <MotionFrame key={motionKey ?? sourceUri} sourceUri={sourceUri} motionKey={motionKey} active={visible && active && foreground} blurred={blurred} opacity={opacity} onVisible={onVisible} />;
}
function MotionFrame({sourceUri,motionKey,active,blurred,opacity,onVisible}:Omit<Props,"visible">) {
 const [ready,setReady]=useState(false),[failed,setFailed]=useState(false);
 // Native status events can repeat. Notify only real transitions so parent
 // state updates cannot remount the cover between poster and motion planes.
 const reported=useRef(false);
 useEffect(()=>{reported.current=false;},[sourceUri]);
 const report=useCallback((next:boolean)=>{if(reported.current===next)return;reported.current=next;onVisible?.(next,motionKey??sourceUri);},[motionKey,onVisible,sourceUri]);
 if(!NativeArtwork || failed)return null;
 return <NativeArtwork sourceUri={sourceUri} active={active} blurred={blurred} pointerEvents="none" style={[StyleSheet.absoluteFill,{opacity:ready?opacity??1:0}]}
  onStatus={event=>{if(event.nativeEvent.uri!==sourceUri)return;if(event.nativeEvent.unavailable){setFailed(true);report(false);}else{setReady(event.nativeEvent.ready);if(event.nativeEvent.ready)report(true);}}} />;
}
