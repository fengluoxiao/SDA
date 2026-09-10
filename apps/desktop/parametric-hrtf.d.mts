export interface SingleHrtfParameters {version:1;itd:"woodworth"|"low-frequency";radius:number;notchHz:number;notchDb:number}
export interface HrtfAnchor {name:string;az:number;el:number;parameters:SingleHrtfParameters}
export interface DirectionalHrtfParameters {version:2;anchors:HrtfAnchor[];power:number}
export type PhrtfParameters=SingleHrtfParameters|DirectionalHrtfParameters;
export function validParameters(value:unknown):value is PhrtfParameters;
export function parameterKey(p:PhrtfParameters):string;
export function generateParameters(random?:()=>number):SingleHrtfParameters;
export function synthesizeHrir(azimuth:number,elevation:number,p:PhrtfParameters):Float32Array;
