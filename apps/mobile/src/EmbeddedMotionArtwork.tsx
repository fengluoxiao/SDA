import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Animated, AppState, StyleSheet, type NativeSyntheticEvent } from "react-native";
import { requireNativeView } from "expo";
import { useNavigationPreferences } from "./navigation-preferences";
export { supportsEmbeddedMotionArtwork } from "./motion-artwork-capability";

type Props = {sourceUri:string;motionKey?:string;nextSourceUri?:string;visible:boolean;active:boolean;blurred?:boolean;opacity?:number;revealDuration?:number;onVisible?(ready:boolean,uri:string):void};
type NativeProps = {sourceUri:string;active:boolean;blurred?:boolean;onStatus(event:NativeSyntheticEvent<{uri:string;ready:boolean;unavailable?:boolean}>):void;style:object;pointerEvents:"none"};
const NativeArtwork=(globalThis as any).expo?.modules?.SdaMotionArtwork
 ? (requireNativeView as unknown as <P>(name:string)=>React.ComponentType<P>)<NativeProps>("SdaMotionArtwork") : null;

// Only the large playback artwork mounts this overlay. Static posters, library
// rows and mini-player artwork stay in their existing Image components.
export function EmbeddedMotionArtwork({sourceUri,motionKey,nextSourceUri,visible,active,blurred,opacity,revealDuration=180,onVisible}:Props) {
 const {motionArtwork}=useNavigationPreferences();
 const [foreground,setForeground]=useState(AppState.currentState==="active");
 useEffect(()=>{const subscription=AppState.addEventListener("change",value=>setForeground(value==="active"));return ()=>subscription.remove();},[]);
 // Keep the native player mounted for tab changes and app backgrounds. The
 // lifecycle/active flags pause it, while its last rendered frame remains.
 if(!motionArtwork || !NativeArtwork)return null;
 const sources=[...new Set([sourceUri,nextSourceUri].filter((uri):uri is string=>!!uri?.startsWith("file:")))];
 return <>{sources.map(uri=><MotionFrame key={uri} sourceUri={uri} motionKey={uri===sourceUri?motionKey??uri:uri} presented={uri===sourceUri} active={uri===sourceUri && visible && active && foreground} blurred={blurred} opacity={opacity} revealDuration={revealDuration} onVisible={onVisible} />)}</>;
}
function MotionFrame({sourceUri,motionKey,presented,active,blurred,opacity,revealDuration,onVisible}:Omit<Props,"visible"> & {presented:boolean}) {
 const [ready,setReady]=useState(false),[failed,setFailed]=useState(false);
 const reveal=useRef(new Animated.Value(0)).current;
 useLayoutEffect(()=>{
  if(revealDuration===0){reveal.setValue(ready?1:0);return;}
  const animation=Animated.timing(reveal,{toValue:ready?1:0,duration:revealDuration??180,useNativeDriver:true,isInteraction:false});
  animation.start();return ()=>animation.stop();
 },[ready,reveal,revealDuration]);
 // Native status events can repeat. Notify only real transitions so parent
 // state updates cannot remount the cover between poster and motion planes.
 const reported=useRef(false);
 useEffect(()=>{reported.current=false;},[sourceUri]);
 const report=useCallback((next:boolean)=>{if(reported.current===next)return;reported.current=next;onVisible?.(next,motionKey??sourceUri);},[motionKey,onVisible,sourceUri]);
 if(!NativeArtwork || failed)return null;
 return <Animated.View pointerEvents="none" style={[StyleSheet.absoluteFill,{opacity:presented?opacity??1:0}]}>
  <Animated.View style={[StyleSheet.absoluteFill,{opacity:reveal}]}>
   <NativeArtwork sourceUri={sourceUri} active={active} blurred={blurred} pointerEvents="none" style={StyleSheet.absoluteFill}
    onStatus={event=>{if(event.nativeEvent.uri!==sourceUri)return;if(event.nativeEvent.unavailable){setFailed(true);report(false);}else{setReady(event.nativeEvent.ready);report(event.nativeEvent.ready);}}} />
  </Animated.View>
 </Animated.View>;
}
