"use strict";
const http = require("node:http");
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { WebSocketServer, createWebSocketStream } = require("ws");
const {HlsPeer}=require("./remote-hls.cjs");

// This HTTP parser receives already-decrypted TLS sockets selected by ALPN.
// No plaintext listener or extra firewall port is created.
function createRemoteWeb(session, {packet,decodePackets}) {
  const hlsPeers=new Map();
  const cookieMatches=(req,hls)=>hls&&String(req.headers.cookie||"").split(";").some(v=>v.trim()===`sda_hls=${hls.cookie}`);
  const authenticate=(req,token)=>session.devices?session.devices.authenticate(req):typeof token==="string"&&/^[a-f0-9]{64}$/.test(token)&&session.key&&crypto.timingSafeEqual(Buffer.from(token,"hex"),session.key)?{legacy:true,canControl:true}:null;
  const identify=(peer,device)=>{if(device&&!device.legacy){peer.deviceId=device.id;peer.deviceName=device.name;peer.canControl=device.canControl!==false;}};
  const mediaAuthorized=(req,peer)=>cookieMatches(req,peer)&&(!session.devices||session.devices.authenticate(req)?.id===peer.deviceId);
  async function pairRequest(req,res){
    if(session.role!=="host"){res.writeHead(404);res.end();return;}
    if(req.url==="/pair/status"&&req.method==="GET"){res.writeHead(200,{"Content-Type":"application/json"});res.end(JSON.stringify(session.devices?.status(req)??{status:"authorized",legacy:true}));return;}
    if(req.url!=="/pair/request"||req.method!=="POST"||req.headers.origin!==`https://${req.headers.host}`){res.writeHead(403);res.end();return;}
    let body=Buffer.alloc(0);for await(const chunk of req){body=Buffer.concat([body,chunk]);if(body.length>2048){res.writeHead(413);res.end();return;}}
    try{const value=JSON.parse(body),result=session.devices?.request(req,session.key,value.token,value.name)??{status:"authorized"};if(result.cookie)res.setHeader("Set-Cookie",result.cookie);res.writeHead(200,{"Content-Type":"application/json"});res.end(JSON.stringify({status:result.status}));}
    catch(e){res.writeHead(403,{"Content-Type":"application/json"});res.end(JSON.stringify({error:e.message}));}
  }
  async function hlsRequest(req,res){
    const requestId=/^\/hls\/([a-f0-9]{32})\//.exec(req.url||"")?.[1];
    const hls=requestId?hlsPeers.get(requestId):[...hlsPeers.values()].find(peer=>cookieMatches(req,peer));
    if(session.role!=="host"){res.writeHead(404);res.end();return;}
    if(session.hooks.hlsAllowed?.()!==true){res.writeHead(403);res.end("电脑端未允许 HLS 原生播放，请刷新网页使用 PCM");return;}
    if(req.url==="/hls/session"&&req.method==="POST"){
      if(req.headers.origin!==`https://${req.headers.host}`){res.writeHead(403);res.end();return;}
      let bytes=Buffer.alloc(0);
      for await(const chunk of req){bytes=Buffer.concat([bytes,chunk]);if(bytes.length>1024){res.writeHead(413);res.end();return;}}
      let auth;try{auth=JSON.parse(bytes);}catch{res.writeHead(400);res.end();return;}
      const device=authenticate(req,auth.token);
      if(!device){res.writeHead(403);res.end();return;}
      const generation=session.generation;
      if(hls&&!hls.destroyed&&mediaAuthorized(req,hls)){
        const previous=hls;await new Promise(resolve=>{previous.once("close",resolve);previous.destroy();});
        await session.detaching;
        if(session.role!=="host"||session.generation!==generation){res.writeHead(409);res.end();return;}
      }
      if(session.hooks.hlsAllowed?.()!==true){res.writeHead(403);res.end("电脑端已关闭 HLS 原生播放");return;}
      if(!session.canAccept()||device.id&&[...session.hostPeers].some(p=>p.deviceId===device.id&&!p.destroyed)){res.writeHead(409);res.end("设备连接数量已达上限，或该设备已在收听");return;}
      const peer=new HlsPeer({id:crypto.randomBytes(16).toString("hex"),address:req.socket.remoteAddress,packet,decodePackets,sync:session.sync,onClose:()=>{hlsPeers.delete(peer.id);session.sync?.remove(peer);},diagnostic:health=>session.hooks.diagnostic?.({deviceId:peer.deviceId??"legacy",...health})});
      session.sync?.add(peer);
      identify(peer,device);hlsPeers.set(peer.id,peer);session.track(peer);session.reservePeer(peer);session.phase="connecting";session.detail="正在准备 Safari 无损媒体流";session.publish();
      const abandoned=()=>{if(!res.writableEnded)peer.destroy();};res.once("close",abandoned);
      try{
        await session.attachHost(peer,session.generation);
        const deadline=Date.now()+12000;while(!peer.sync&&!peer.destroyed&&!peer.playlist()&&Date.now()<deadline)await new Promise(r=>setTimeout(r,25));
        if(peer.destroyed||!peer.sync&&!peer.playlist())throw Error("HLS 音频未就绪");
        res.setHeader("Set-Cookie",`sda_hls=${peer.cookie}; Path=/; Secure; HttpOnly; SameSite=Strict`);
        res.writeHead(200,{"Content-Type":"application/json"});res.end(JSON.stringify({id:peer.id,stream:peer.mediaInfo().stream,format:"48 kHz · 24-bit FLAC"}));
      }catch(e){session.failPeer(peer,e.message);res.writeHead(503);res.end("无法建立 HLS 收听会话");}return;
    }
    const control=/^\/hls\/([a-f0-9]{32})\/control$/.exec(req.url||"");
    if(control&&req.method==="POST"){
      if(req.headers.origin!==`https://${req.headers.host}`||!hls||hls.id!==control[1]||!mediaAuthorized(req,hls)){res.writeHead(403);res.end();return;}
      const peer=hls;let bytes=Buffer.alloc(0);for await(const chunk of req){bytes=Buffer.concat([bytes,chunk]);if(bytes.length>65536){res.writeHead(413);res.end();return;}}
      if(peer.destroyed){res.writeHead(410);res.end();return;}
      const reply=await peer.requestControl(bytes);res.writeHead(200,{"Content-Type":"application/json"});res.end(JSON.stringify(reply));return;
    }
    const stopping=/^\/hls\/([a-f0-9]{32})\/stop$/.exec(req.url||"");
    if(stopping&&req.method==="POST"){
      if(req.headers.origin!==`https://${req.headers.host}`||!hls||hls.id!==stopping[1]||!mediaAuthorized(req,hls)){res.writeHead(403);res.end();return;}
      hls.destroy();res.writeHead(204);res.end();return;
    }
    const mediaUrl=new URL(req.url,'https://localhost');
    const match=/^\/hls\/([a-f0-9]{32})\/(?:e(\d+)\/)?(index\.m3u8|init\.mp4|\d+(?:\.\d+)?\.m4s)$/.exec(mediaUrl.pathname);
    if(!match||!hls||hls.id!==match[1]||!mediaAuthorized(req,hls)){res.writeHead(403);res.end();return;}
    if(!["GET","HEAD"].includes(req.method)){res.writeHead(405);res.end();return;}
    const peer=hls,name=match[3];
    if((match[2]===undefined&&peer.epoch!==0)||(match[2]!==undefined&&Number(match[2])!==peer.epoch)){res.writeHead(410);res.end();return;}
    const epoch=peer.epoch,part=/^(\d+)\.(\d+)\.m4s$/.exec(name);
    const msn=mediaUrl.searchParams.get('_HLS_msn'),requestedPart=mediaUrl.searchParams.get('_HLS_part');
    if(msn!==null&&(!/^\d+$/.test(msn)||Number(msn)>peer.sequence+2)||requestedPart!==null&&(msn===null||!/^\d+$/.test(requestedPart)||Number(requestedPart)>5)) {res.writeHead(400);res.end();return;}
    if(part||msn!==null){
      peer.lowLatency=true;
      if((peer.blockingRequests??0)>=8){res.writeHead(429);res.end();return;}
      peer.blockingRequests=(peer.blockingRequests??0)+1;
      try{
        const deadline=Date.now()+3000;
        while(!peer.destroyed&&!res.destroyed&&peer.epoch===epoch&&Date.now()<deadline){
          const available=part?peer.parts.some(p=>p.sequence===Number(part[1])&&p.index===Number(part[2])):
            requestedPart!==null?peer.sequence>Number(msn)||peer.parts.some(p=>p.sequence===Number(msn)&&p.index>=Number(requestedPart)):peer.sequence>Number(msn);
          if(available||peer.programEnded)break;
          await new Promise(resolve=>setTimeout(resolve,20));
        }
      }finally{peer.blockingRequests--;}
      if(res.destroyed)return;
      if(peer.destroyed||peer.epoch!==epoch){res.writeHead(410);res.end();return;}
    }
    let body=name==="index.m3u8"?peer.playlist():name==="init.mp4"?peer.init:part?peer.parts.find(p=>p.sequence===Number(part[1])&&p.index===Number(part[2]))?.bytes:peer.segments.find(v=>`${v.sequence}.m4s`===name)?.bytes;
    if(!body){res.writeHead(404);res.end();return;}
    peer.touch();res.setHeader("Content-Type",name.endsWith("m3u8")?"application/vnd.apple.mpegurl":"audio/mp4");
    // Media bytes are immutable within a unique session/epoch. Permit short
    // private caching so paused receivers can warm upcoming segments. Control,
    // playlists, credentials and failed authorization responses stay no-store.
    if(!name.endsWith('m3u8'))res.setHeader('Cache-Control','private, max-age=30');
    // Apple media loaders may request byte ranges even for independent segments.
    if(req.headers.range){const range=/^bytes=(\d+)-(\d*)$/.exec(req.headers.range);const start=range?Number(range[1]):-1,end=range&&range[2]?Number(range[2]):body.length-1;
      if(start<0||start>=body.length||end<start||end>=body.length){res.writeHead(416,{"Content-Range":`bytes */${body.length}`});res.end();return;}
      res.writeHead(206,{"Accept-Ranges":"bytes","Content-Range":`bytes ${start}-${end}/${body.length}`,"Content-Length":end-start+1});res.end(req.method==="HEAD"?undefined:body.subarray(start,end+1));return;}
    res.writeHead(200,{"Content-Length":body.length,"Accept-Ranges":"bytes"});res.end(req.method==="HEAD"?undefined:body);
  }
  const files = new Map([
    ["/", ["index.html", "text/html; charset=utf-8"]],
    ["/swiper.mjs", ["swiper.mjs", "text/javascript; charset=utf-8"]],
    ["/swiper.css", ["swiper.css", "text/css; charset=utf-8"]],
    ["/scene.mjs", ["scene.mjs", "text/javascript; charset=utf-8"]],
    ["/scene-view.mjs", ["scene-view.mjs", "text/javascript; charset=utf-8"]],
    ["/pages.mjs", ["pages.mjs", "text/javascript; charset=utf-8"]],
    ["/app.mjs", ["app.mjs", "text/javascript; charset=utf-8"]],
    ["/playback-sync.mjs", ["playback-sync.mjs", "text/javascript; charset=utf-8"]],
    ["/pause-prefetch.mjs", ["pause-prefetch.mjs", "text/javascript; charset=utf-8"]],
    ["/more-menu.mjs", ["more-menu.mjs", "text/javascript; charset=utf-8"]],
    ["/motion.mjs", ["motion.mjs", "text/javascript; charset=utf-8"]],
    ["/synchronized-playback.mjs", ["synchronized-playback.mjs", "text/javascript; charset=utf-8"]],
    ["/media.mjs", ["media.mjs", "text/javascript; charset=utf-8"]],
    ["/tools.mjs", ["tools.mjs", "text/javascript; charset=utf-8"]],
    ["/phrtf-test.mjs", ["phrtf-test.mjs", "text/javascript; charset=utf-8"]],
    ["/phrtf-core.mjs", ["phrtf-core.mjs", "text/javascript; charset=utf-8"]],
    ["/app.css", ["app.css", "text/css; charset=utf-8"]],
    ["/pcm-worklet.mjs", ["pcm-worklet.mjs", "text/javascript; charset=utf-8"]],
    ["/pcm-buffer.mjs", ["pcm-buffer.mjs", "text/javascript; charset=utf-8"]],
  ]);
  files.set("/startup-calibration.mjs",["startup-calibration.mjs","text/javascript; charset=utf-8"]);
  files.set("/pcm-media-output.mjs",["pcm-media-output.mjs","text/javascript; charset=utf-8"]);
  const cache = new Map();
  const server = http.createServer({ maxHeaderSize: 8192 }, (req, res) => {
    res.setHeader("Cache-Control", "no-store");
    res.setHeader("Referrer-Policy", "no-referrer");
    res.setHeader("X-Content-Type-Options", "nosniff");
    res.setHeader("Content-Security-Policy", "default-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self'; connect-src 'self' wss:; worker-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'");
    if(req.url?.startsWith("/pair/")){void pairRequest(req,res).catch(()=>{if(!res.headersSent)res.writeHead(500);res.end();});return;}
    if(req.url?.startsWith("/hls/")){void hlsRequest(req,res).catch(()=>{if(!res.headersSent)res.writeHead(500);res.end();});return;}
    if (req.method !== "GET" && req.method !== "HEAD") { res.writeHead(405); res.end(); return; }
    if (req.url === "/favicon.ico") { res.writeHead(204); res.end(); return; }
    if(req.url==="/playback-policy.mjs"&&session.role==="host"){
      res.writeHead(200,{"Content-Type":"text/javascript; charset=utf-8"});
      res.end(`export const hlsAllowedByHost = ${session.hooks.hlsAllowed?.()===true};`);return;
    }
    const asset = files.get(req.url);
    if (!asset || session.role !== "host") { res.writeHead(404); res.end(); return; }
    try {
      if (!cache.has(asset[0])) cache.set(asset[0], fs.readFileSync(path.join(__dirname, "remote-web", asset[0])));
      const body = cache.get(asset[0]);
      res.writeHead(200, { "Content-Type": asset[1], "Content-Length": body.length });
      res.end(req.method === "HEAD" ? undefined : body);
    } catch { res.writeHead(503); res.end("Web client assets unavailable"); }
  });
  server.headersTimeout = 5000; server.requestTimeout = 10000; server.keepAliveTimeout = 5000;
  server.on("clientError", (_error, socket) => socket.destroy());
  const wsServer = new WebSocketServer({ noServer: true, maxPayload: 128 * 1024, perMessageDeflate: false });
  server.on("upgrade", (req, socket, head) => {
    if (req.url !== "/stream" || req.headers.origin !== `https://${req.headers.host}` || session.role !== "host") {
      socket.end("HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n"); return;
    }
    wsServer.handleUpgrade(req, socket, head, ws => {
      const generation = session.generation;
      const timer = setTimeout(() => ws.close(1008, "配对超时"), 5000).unref();
      ws.once("close", () => clearTimeout(timer)); ws.on("error", () => {});
      ws.once("message", (data, binary) => {
        clearTimeout(timer);
        let auth;
        try { if (binary) throw Error(); auth = JSON.parse(data.toString("utf8")); } catch { ws.close(1008, "配对信息无效"); return; }
        const device=authenticate(req,auth.token);
        if (session.role !== "host" || session.generation !== generation || auth.protocol !== 1 || !device) {
          ws.close(1008, "配对地址已失效或密钥错误"); return;
        }
        // Same slot as the native client, acquired synchronously after auth.
        if(auth.hls){
          const hls=hlsPeers.get(auth.hls);
          if(!hls||auth.hls!==hls.id||!mediaAuthorized(req,hls)){ws.close(1008,"收听会话已失效");return;}
          const stream=createWebSocketStream(ws,{highWaterMark:64*1024});stream.setNoDelay=()=>stream;session.track(stream);
          try{hls.attachControl(stream,decodePackets);}catch{ws.close(1008,"此收听会话已有控制页面");}return;
        }
        if (!session.canAccept()||device.id&&[...session.hostPeers].some(p=>p.deviceId===device.id&&!p.destroyed)) { ws.close(1008, "设备连接数量已达上限，或该设备已在收听"); return; }
        const stream = createWebSocketStream(ws, { highWaterMark: 64 * 1024 });
        stream.remoteAddress = socket.remoteAddress;
        stream.setNoDelay = () => stream;
        identify(stream,device);session.track(stream);session.reservePeer(stream);session.phase = "connecting";
        session.detail = "浏览器已配对，正在连接音频"; session.publish();
        void session.attachHost(stream, generation).catch(error => session.failPeer(stream, error.message));
      });
    });
  });
  return {
    disableHls() { for(const peer of hlsPeers.values())peer.destroy(); },
    accept(socket) { server.emit("connection", socket); },
    close() { for(const peer of hlsPeers.values())peer.destroy();hlsPeers.clear();for (const client of wsServer.clients) client.terminate(); wsServer.close(); server.close(); cache.clear(); },
  };
}
module.exports = { createRemoteWeb };
