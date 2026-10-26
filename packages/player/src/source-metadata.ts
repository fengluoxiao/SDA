import fs from "node:fs";
import path from "node:path";
import {createHash} from "node:crypto";
import {parseFile} from "music-metadata";

const MAX_ASSET = 16 * 1024 * 1024;
export async function readSourceMetadata(inputPath:string, check:()=>void) {
  check();
  const parsed = await parseFile(inputPath, {duration:false, skipCovers:false});
  check();
  // Some WAV writers emit UTF-8 INFO text without a CSET declaration. Recover
  // only losslessly decodable UTF-8, leaving ordinary Latin-1 and raw tags intact.
  for(const tag of parsed.native.exif??[]) {
    if(typeof tag.value!=="string"||!/[\x80-\xff]/.test(tag.value)||[...tag.value].some(char=>char.charCodeAt(0)>255))continue;
    let recovered:string;
    try {recovered=new TextDecoder("utf-8",{fatal:true}).decode(Buffer.from(tag.value,"latin1"));}catch{continue;}
    const common=parsed.common as unknown as Record<string,unknown>;
    for(const [key,value]of Object.entries(common)) {
      if(value===tag.value)common[key]=recovered;
      else if(Array.isArray(value))common[key]=value.map(item=>item===tag.value?recovered:item);
    }
  }
  const blobs = new Map<string,{sha256:string;data:Buffer;mimeType:string;filename:string;kind:string}>();
  const add = (value:Uint8Array,mimeType:string,filename:string,kind:string) => {
    if(value.length>MAX_ASSET)throw Error("歌曲元数据附件超过 16 MiB，未写入目标文件");
    const data=Buffer.from(value),sha256=createHash("sha256").update(data).digest("hex");
    const existing=blobs.get(sha256);
    if(!existing||existing.kind==="binary")blobs.set(sha256,{sha256,data,mimeType,filename,kind});
    return sha256;
  };
  const pictures=(parsed.common.picture??[]).map((picture,i)=>({
    ...picture,sha256:add(picture.data,picture.format,`cover-${i+1}`,"picture"),
  }));
  const lyrics:{format:string;text:string;data:Buffer;filename:string;language:string;source:string;sha256:string}[]=[];
  function addLyrics(text:string,data:Buffer,filename:string,language:string,source:string) {
    const format=/\.ttml$/i.test(filename)||/<(?:\w+:)?tt(?:\s|>)/i.test(text)?"ttml":/\.lrc$/i.test(filename)||/^\s*\[(?:\d+:\d+|ar:|ti:|al:|by:)/m.test(text)?"lrc":"text";
    const name=filename||`lyrics-${lyrics.length+1}.${format==="text"?"txt":format}`;
    if(source==="embedded"&&lyrics.some(value=>value.source==="embedded"&&value.text===text&&value.language===language))return;
    const mimeType=format==="ttml"?"application/ttml+xml":"text/plain";
    lyrics.push({format,text,data,filename:name,language,source,sha256:add(data,mimeType,name,"lyrics")});
  }
  // The common-tag adapter parses LRC into text lines. Prefer native strings
  // so the original timestamps, XML and whitespace remain recoverable.
  for(const tags of Object.values(parsed.native))for(const tag of tags) {
    if(!/^(?:©lyr|USLT(?::.*)?|LYRICS|UNSYNCEDLYRICS|TTML|TXXX:(?:LYRICS|TTML)|----:.*(?:LYRICS|TTML))$/i.test(tag.id))continue;
    const value=tag.value as {text?:unknown;language?:string}|string;
    const text=typeof value==="string"?value:typeof value?.text==="string"?value.text:undefined;
    if(text)addLyrics(text,Buffer.from(text,"utf8"),/TTML$/i.test(tag.id)?"lyrics.ttml":"",typeof value==="object"?value.language??"und":"und","embedded");
  }
  if(!lyrics.length)for(const lyric of parsed.common.lyrics??[]) {
    const text=lyric.text??lyric.syncText?.map(line=>line.text).join("\n");
    if(text)addLyrics(text,Buffer.from(text,"utf8"),"",lyric.language??"und","embedded");
  }
  // Only exact same-stem companions belong to this source. Keep both formats,
  // original bytes/BOM and timing markup; never fetch or rewrite lyric files.
  const directory=path.dirname(inputPath),stem=path.parse(inputPath).name.toLowerCase();
  for(const entry of fs.readdirSync(directory,{withFileTypes:true})) {
    if(!entry.isFile())continue;
    const name=path.parse(entry.name);
    if(name.name.toLowerCase()!==stem||!/^\.(lrc|ttml)$/i.test(name.ext))continue;
    check();
    const file=path.join(directory,entry.name),before=fs.statSync(file);
    if(before.size>MAX_ASSET)throw Error("歌词文件超过 16 MiB，未写入目标文件");
    const data=fs.readFileSync(file),after=fs.statSync(file);
    if(before.size!==after.size||before.mtimeMs!==after.mtimeMs)throw Error("读取期间歌词发生变化，请重新导出");
    let text:string;
    if(data[0]===0xff&&data[1]===0xfe)text=new TextDecoder("utf-16le").decode(data);
    else if(data[0]===0xfe&&data[1]===0xff)text=new TextDecoder("utf-16be").decode(data);
    else {try{text=new TextDecoder("utf-8",{fatal:true}).decode(data);}catch{text=new TextDecoder("gb18030").decode(data);}}
    addLyrics(text,data,entry.name,"und","sidecar");
  }
  const preserve = (value:unknown):unknown => {
    if(value instanceof Uint8Array)return {asset:add(value,"application/octet-stream","source-tag.bin","binary")};
    if(typeof value==="bigint")return value.toString();
    if(Array.isArray(value))return value.map(preserve);
    if(value&&typeof value==="object")return Object.fromEntries(Object.entries(value).map(([key,item])=>[key,preserve(item)]));
    return value;
  };
  const source={schema:"sda.source-metadata",version:1,filename:path.basename(inputPath),
    common:preserve(parsed.common),native:preserve(parsed.native),format:preserve(parsed.format),
    lyrics:lyrics.map(({data,text,...lyric})=>({...lyric,asset:lyric.sha256})),
    assets:[] as unknown[],
  };
  source.assets=[...blobs.values()].map(({data,...asset})=>({...asset,bytes:data.length}));
  check();
  return {common:parsed.common,pictures,lyrics,source,blobs:[...blobs.values()]};
}
