import {PCFShadowMap, WebGLRenderer} from 'three';
import type {GLProps} from '@react-three/fiber';
import type {WebGPURenderer} from 'three/webgpu';
type DefaultGLProps=Parameters<Extract<GLProps,(defaults:never)=>unknown>>[0];

// R3F's boolean default selects removed PCFSoftShadowMap on every configure.
// Keep shadows disabled without repeatedly invoking WebGPU's warning setter.
export const spatialShadows = {enabled: false, type: PCFShadowMap};

/** Keep the explicit software renderer usable on machines without WebGPU. */
export function spatialRenderer(software=false,lowPower=software){
 return async(defaults:DefaultGLProps)=>{
  const canvas=defaults.canvas as HTMLCanvasElement;
  const options={...defaults,canvas,antialias:!lowPower,powerPreference:lowPower?'low-power' as const:'high-performance' as const};
  if(!software&&'gpu' in navigator&&navigator.gpu){
   let candidate:WebGPURenderer|undefined;
   try{
    const {WebGPURenderer}=await import('three/webgpu');
    const renderer=candidate=new WebGPURenderer({...options,trackTimestamp:true});
    await renderer.init();
    const backend=renderer.backend as unknown as {isWebGPUBackend?:boolean;trackTimestamp:boolean};
    canvas.dataset.sdaRenderer=backend.isWebGPUBackend?'webgpu':'webgl2';
    // Enable timestamp recording only while the performance monitor is active.
    canvas.dataset.sdaGpuTimer=String(backend.trackTimestamp);
    backend.trackTimestamp=false;
    return renderer;
   }catch(error){
    candidate?.dispose();
    console.warn('SDA WebGPU initialization failed; trying WebGL.',error);
   }
  }
  const renderer=new WebGLRenderer(options);
  canvas.dataset.sdaRenderer='webgl2';
  return renderer;
 };
}
