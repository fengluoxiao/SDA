"use strict";
class NativeFilePlayback {
  constructor({send,publish}) { this.send=send;this.publish=publish;this.pending=new Map();this.next=1;this.supported=false; }
  handle(data) {
    if(!data||typeof data!=="object")return;
    if(data.kind==="reply") {
      const item=this.pending.get(data.requestId);if(!item)return;
      this.pending.delete(data.requestId);clearTimeout(item.timer);
      if(data.session!==item.session){item.reject(Error("Native file session mismatch"));return;}
      if(data.ok===true)item.resolve(data.value);else item.reject(Error(String(data.error||"Native file operation failed")));
    } else if(data.kind==="state"||data.kind==="error")this.publish(data);
  }
  request(command) {
    if(!this.supported)return Promise.reject(Error("请重启安装了原生文件解码器的桌面版本"));
    if(!command||!["metadata","probe","open","seek","pause","configure","close","status"].includes(command.action)||typeof command.session!=="string"||command.session.length>128)return Promise.reject(Error("Invalid native file command"));
    if(["open","probe"].includes(command.action)&&(typeof command.path!=="string"||!command.path||command.path.length>32768))return Promise.reject(Error("Invalid native source path"));
    if(command.action==="seek"&&(!Number.isFinite(command.positionMs)||command.positionMs<0))return Promise.reject(Error("Invalid native seek time"));
    const requestId=this.next++,type="file"+command.action[0].toUpperCase()+command.action.slice(1);
    return new Promise((resolve,reject)=>{
      const timer=setTimeout(()=>{this.pending.delete(requestId);reject(Error("Native file operation timed out"));},["open","probe"].includes(command.action)?60000:15000);
      this.pending.set(requestId,{resolve,reject,timer,session:command.session});
      const {action,...value}=command;
      if(!this.send({...value,type,requestId})){clearTimeout(timer);this.pending.delete(requestId);reject(Error("Native file transport is unavailable"));}
    });
  }
  reset(reason="Native file transport closed") { for(const item of this.pending.values()){clearTimeout(item.timer);item.reject(Error(reason));}this.pending.clear();this.supported=false; }
}
module.exports={NativeFilePlayback};
