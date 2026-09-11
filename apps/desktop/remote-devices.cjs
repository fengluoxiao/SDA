"use strict";
const crypto=require('node:crypto');
const hash=value=>crypto.createHash('sha256').update(value).digest('hex');
const cookie=req=>String(req.headers.cookie||'').split(';').map(v=>v.trim()).find(v=>v.startsWith('sda_device='))?.slice(11)||'';
class RemoteDevices {
 constructor({read,write,changed,kick}){this.read=read;this.write=write;this.changed=changed;this.kick=kick;this.pending=new Map();this.attempts=new Map();}
 records(){const values=this.read();return (Array.isArray(values)?values:[]).filter(v=>v&&typeof v.id==='string'&&typeof v.name==='string'&&/^[a-f0-9]{64}$/.test(v.hash)).slice(0,16);}
 list(){return this.records().map(({hash,...v})=>v);}
 clean(){const now=Date.now();for(const [id,v]of this.pending)if(v.expires<now)this.pending.delete(id);for(const [ip,v]of this.attempts)if(v.until<now)this.attempts.delete(ip);}
 pendingList(){this.clean();return [...this.pending.values()].map(({hash,...v})=>v);}
 authenticateToken(token){if(typeof token!=='string'||!/^[a-f0-9]{64}$/.test(token))return null;const digest=hash(token);return this.records().find(v=>crypto.timingSafeEqual(Buffer.from(v.hash,'hex'),Buffer.from(digest,'hex')))||null;}
 authenticate(req){return this.authenticateToken(cookie(req));}
 status(req){this.clean();const device=this.authenticate(req);if(device)return {status:'authorized',name:device.name,canControl:device.canControl!==false};const digest=hash(cookie(req));const pending=[...this.pending.values()].find(v=>v.hash===digest);return {status:pending?'pending':'unpaired'};}
 request(req,key,token,name){
  this.clean();if(this.authenticate(req))return {status:'authorized'};
  const current=this.status(req);if(current.status==='pending')return current;
  const ip=req.socket.remoteAddress||'unknown';const attempts=this.attempts.get(ip)||{count:0,until:Date.now()+60000};
  if(this.attempts.size>=256&&!this.attempts.has(ip))throw Error('配对请求过多，请稍后重试');
  this.attempts.set(ip,attempts);if(++attempts.count>5)throw Error('配对尝试过多，请一分钟后重试');
  if(typeof token!=='string'||!/^[a-f0-9]{64}$/.test(token)||!key||!crypto.timingSafeEqual(Buffer.from(token,'hex'),key))throw Error('配对密钥错误');
  if(this.pending.size>=4||this.records().length>=16)throw Error('授权设备或待处理申请已达到上限');
  const secret=crypto.randomBytes(32).toString('hex'),id=crypto.randomUUID();
  this.pending.set(id,{id,hash:hash(secret),name:typeof name==='string'?name.replace(/[\x00-\x1f]/g,'').trim().slice(0,60)||'浏览器设备':'浏览器设备',address:ip,expires:Date.now()+120000});this.changed();
  return {status:'pending',cookie:`sda_device=${secret}; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=31536000`};
 }
 approve(id,canControl){this.clean();const pending=this.pending.get(id);if(!pending)throw Error('配对申请已过期');if(this.records().length>=16)throw Error('已授权设备达到上限，请先撤销旧设备');if(typeof canControl!=='boolean')throw Error('无效设备权限');const {address,expires,...record}=pending;this.write([...this.records(),{...record,canControl,createdAt:Date.now()}]);this.pending.delete(id);this.changed();}
 reject(id){this.pending.delete(id);this.changed();}
 revoke(id){this.write(this.records().filter(v=>v.id!==id));this.kick(id);this.changed();}
 permission(id,canControl){if(typeof canControl!=='boolean')throw Error('无效设备权限');this.write(this.records().map(v=>v.id===id?{...v,canControl}:v));this.kick(id);this.changed();}
 clearPending(){this.pending.clear();this.attempts.clear();}
}
module.exports={RemoteDevices};
