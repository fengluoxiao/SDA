'use strict';
const path=require('node:path');
const {execFile}=require('node:child_process');
const layouts=require('../windows-system-audio/layouts.json');
function inputFormat(layout) {
  if(!Object.hasOwn(layouts,layout))throw Error('不支持的系统输入布局');
  // Use standard Windows positions where the driver advertises an exact match.
  // Other layouts use the explicit SDA discrete-order contract.
  const masks={'2.0':3,'5.1':0x60f,'7.1.4':0x2d63f};
  return {channels:layouts[layout].length,mask:masks[layout]??0};
}
async function configureInput(device,layout) {
  if(!device?.available || !/SDA (?:Spatial Bitstream|HDMI|Virtual HDMI)/.test(device.name) || !/^\{0\.0\.0\.00000000\}\.\{[a-f0-9-]{36}\}$/i.test(device.id))throw Error('未找到可配置的 SDA 虚拟输入');
  const {channels,mask}=inputFormat(layout);
  // Windows accepts these values in PolicyConfig but rejects shared-client
  // Initialize on this endpoint above twelve channels. Keep its working mix
  // intact; the driver still accepts these exact discrete formats exclusively.
  if(channels>12) return {channels,mask,shared:false};
  await new Promise((resolve,reject)=>execFile('powershell.exe',['-NoProfile','-File',path.resolve(__dirname,'../windows-system-audio/set-input-format.ps1'),'-DeviceId',device.id,'-Channels',String(channels),'-Mask',String(mask)],{windowsHide:true,timeout:15000,maxBuffer:65536},(error,stdout,stderr)=>error?reject(Error(`系统输入格式配置失败，未开始接收：${String(stderr||error.message).slice(0,800)}`)):resolve(stdout)));
  return {channels,mask,shared:true};
}
async function discoverEndpoints({repair=false} = {}) {
  const result=await new Promise((resolve,reject)=>execFile('powershell.exe',['-NoProfile','-File',path.resolve(__dirname,'../windows-system-audio/configure-endpoints.ps1'),...(repair?[]:['-DiscoverOnly'])],{windowsHide:true,timeout:15000,maxBuffer:65536},(error,stdout,stderr)=>error?reject(Error(String(stderr||error.message))):resolve(stdout)));
  return JSON.parse(result);
}
module.exports={inputFormat,configureInput,discoverEndpoints};
