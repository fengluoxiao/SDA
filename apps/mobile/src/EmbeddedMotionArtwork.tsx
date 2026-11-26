import React, { useCallback, useEffect, useRef, useState } from "react";
import { AppState, StyleSheet, View, type NativeSyntheticEvent } from "react-native";
import { requireNativeView } from "expo";
import { useNavigationPreferences } from "./navigation-preferences";
export { supportsEmbeddedMotionArtwork } from "./motion-artwork-capability";

type Props = {sourceUri:string;motionKey?:string;nextSourceUri?:string;visible:boolean;active:boolean;blurred?:boolean;bottomExtension?:boolean;opacity?:number;onVisible?(ready:boolean,uri:string):void;onUnavailable?(uri:string):void};
type NativeProps = {sourceUri:string;presented:boolean;active:boolean;blurred?:boolean;bottomExtension:boolean;onStatus(event:NativeSyntheticEvent<{uri:string;ready:boolean;unavailable?:boolean}>):void;style:object;pointerEvents:"none"};
const NativeArtwork=(globalThis as any).expo?.modules?.SdaMotionArtwork
 ? (requireNativeView as unknown as <P>(name:string)=>React.ComponentType<P>)<NativeProps>("SdaMotionArtwork") : null;

// Only the large playback artwork mounts this overlay. Static posters, library
// rows and mini-player artwork stay in their existing Image components.
export function EmbeddedMotionArtwork({sourceUri,motionKey,nextSourceUri,visible,active,blurred,bottomExtension=false,opacity,onVisible,onUnavailable}:Props) {
 const {motionArtwork}=useNavigationPreferences();
 const [foreground,setForeground]=useState(AppState.currentState==="active");
 useEffect(()=>{const subscription=AppState.addEventListener("change",value=>setForeground(value==="active"));return ()=>subscription.remove();},[]);
 // Keep the native player mounted for tab changes and app backgrounds. The
 // lifecycle/active flags pause it, while its last rendered frame remains.
 if(!motionArtwork || !NativeArtwork)return null;
 const sources=[...new Set([sourceUri,nextSourceUri].filter((uri):uri is string=>!!uri?.startsWith("file:")))];
 return <>{sources.map(uri=><MotionFrame key={uri} sourceUri={uri} motionKey={uri===sourceUri?motionKey??uri:uri} presented={uri===sourceUri} active={uri===sourceUri && visible && active && foreground} blurred={blurred} bottomExtension={bottomExtension} opacity={opacity} onVisible={onVisible} onUnavailable={onUnavailable} />)}</>;
}
function MotionFrame({sourceUri,motionKey,presented,active,blurred,bottomExtension=false,opacity,onVisible,onUnavailable}:Omit<Props,"visible"> & {presented:boolean}) {
 const [ready,setReady]=useState(false),[failed,setFailed]=useState(false);
 // Native status events can repeat. Notify only real transitions so parent
 // state updates cannot remount the cover between poster and motion planes.
 const reported=useRef(false);
 useEffect(()=>{reported.current=false;},[sourceUri]);
 const report=useCallback((next:boolean)=>{if(reported.current===next)return;reported.current=next;onVisible?.(next,motionKey??sourceUri);},[motionKey,onVisible,sourceUri]);
 if(!NativeArtwork || failed)return null;
 return <View pointerEvents="none" style={[StyleSheet.absoluteFill,{opacity:presented&&ready?opacity??1:0}]}>
  <NativeArtwork sourceUri={sourceUri} presented={presented} active={active} blurred={blurred} bottomExtension={bottomExtension} pointerEvents="none" style={StyleSheet.absoluteFill}
   onStatus={event=>{if(event.nativeEvent.uri!==sourceUri)return;if(event.nativeEvent.unavailable){setFailed(true);report(false);onUnavailable?.(sourceUri);}else{setReady(event.nativeEvent.ready);report(event.nativeEvent.ready);}}} />
 </View>;
}
