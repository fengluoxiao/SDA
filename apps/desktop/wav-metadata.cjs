const MAX_METADATA = 64 * 1024 * 1024;
function chunk(id,body) {
  if(body.length>MAX_METADATA)throw Error('歌曲元数据超过 64 MiB，未写入目标文件');
  const bytes=Buffer.alloc(8+body.length+(body.length&1));
  bytes.write(id,0,4,'ascii');bytes.writeUInt32LE(body.length,4);body.copy(bytes,8);return bytes;
}
function syncsafe(value) {
  if(!Number.isSafeInteger(value)||value<0||value>0x0fffffff)throw Error('ID3 元数据过大');
  return Buffer.from([(value>>>21)&127,(value>>>14)&127,(value>>>7)&127,value&127]);
}
const utf8=value=>Buffer.from(String(value),'utf8');
const zero=Buffer.from([0]);
const textFrame=(id,value)=>value===undefined||value===null||value===''?null:frame(id,Buffer.concat([Buffer.from([3]),utf8(Array.isArray(value)?value.join('\0'):value)]));
function frame(id,payload) {return Buffer.concat([Buffer.from(id,'ascii'),syncsafe(payload.length),Buffer.alloc(2),payload]);}
function objectFrame(mime,filename,description,data) {
  return frame('GEOB',Buffer.concat([Buffer.from([3]),utf8(mime),zero,utf8(filename),zero,utf8(description),zero,Buffer.from(data)]));
}
function buildMetadataChunks(metadata,render) {
  const common=metadata.common,frames=[];
  const emit=(id,value)=>{const encoded=textFrame(id,value);if(encoded)frames.push(encoded);};
  const number=value=>value?.no?String(value.no)+(value.of?'/'+value.of:''):undefined;
  emit('TIT2',common.title);emit('TPE1',common.artists?.length?common.artists:common.artist);
  emit('TPE2',common.albumartists?.length?common.albumartists:common.albumartist);emit('TALB',common.album);
  emit('TDRC',common.date??common.year);emit('TDOR',common.originaldate??common.originalyear);
  emit('TCON',common.genre);emit('TCOM',common.composer);emit('TEXT',common.lyricist);
  emit('TSOA',common.albumsort);emit('TSOP',common.artistsort);emit('TSOT',common.titlesort);emit('TSOC',common.composersort);
  emit('TPE3',common.conductor);emit('TPE4',common.remixer);emit('TBPM',common.bpm);emit('TMOO',common.mood);emit('TKEY',common.key);emit('TLAN',common.language);
  emit('TRCK',number(common.track));emit('TPOS',number(common.disk));emit('TCOP',common.copyright);
  emit('TPUB',common.label??common.publisher);emit('TSRC',common.isrc);emit('TENC','SDA');
  for(const comment of common.comment??[])if(comment.text)frames.push(frame('COMM',Buffer.concat([Buffer.from([3]),Buffer.from('und'),utf8(comment.descriptor??''),zero,utf8(comment.text)])));
  const used=new Set();
  for(const picture of metadata.pictures) {
    const type=/back/i.test(picture.type??'')?4:/front|cover/i.test(picture.type??'')?3:metadata.pictures.length===1?3:0;
    frames.push(frame('APIC',Buffer.concat([Buffer.from([3]),utf8(picture.format),zero,Buffer.from([type]),utf8(picture.description??''),zero,Buffer.from(picture.data)])));
    used.add(picture.sha256);
  }
  for(const [i,lyric]of metadata.lyrics.entries()) {
    const language=/^[a-z]{3}$/i.test(lyric.language)?lyric.language:'und';
    frames.push(frame('USLT',Buffer.concat([Buffer.from([3]),Buffer.from(language),utf8(lyric.filename),zero,utf8(lyric.text)])));
    const mime=lyric.format==='ttml'?'application/ttml+xml':'text/plain';
    frames.push(objectFrame(mime,lyric.filename,'SDA lyrics '+(i+1)+'; '+lyric.format,lyric.data));used.add(lyric.sha256);
  }
  for(const asset of metadata.blobs)if(!used.has(asset.sha256))frames.push(objectFrame(asset.mimeType,asset.filename,'SDA source asset '+asset.sha256,asset.data));
  frames.push(objectFrame('application/json','source-metadata.json','SDA source metadata v1',utf8(JSON.stringify(metadata.source))));
  if(render)frames.push(objectFrame('application/json','render-metadata.json','SDA render metadata v1',utf8(JSON.stringify(render))));
  const payload=Buffer.concat(frames),id3=chunk('id3 ',Buffer.concat([Buffer.from('ID3'),Buffer.from([4,0,0]),syncsafe(payload.length),payload]));
  const info=[];
  const fields={INAM:common.title,IART:common.artist,IPRD:common.album,ICRD:common.date??common.year,IGNR:common.genre?.join('; '),ITRK:number(common.track),ICOP:common.copyright};
  for(const [id,value]of Object.entries(fields))if(value!==undefined&&value!==null&&value!=='')info.push(chunk(id,Buffer.concat([utf8(value),zero])));
  const charset=Buffer.alloc(8);charset.writeUInt16LE(65001,0);
  const bytes=Buffer.concat([...(info.length?[chunk('CSET',charset),chunk('LIST',Buffer.concat([Buffer.from('INFO'),...info]))]:[]),id3]);
  if(bytes.length>MAX_METADATA)throw Error('歌曲元数据超过 64 MiB，未写入目标文件');
  return {bytes,summary:{schemaVersion:1,fields:Object.entries(fields).filter(([,value])=>value!==undefined&&value!==null&&value!=='').map(([key])=>key),coverCount:metadata.pictures.length,lyrics:metadata.lyrics.map(lyric=>({format:lyric.format,source:lyric.source,filename:lyric.filename}))}};
}
module.exports={chunk,buildMetadataChunks};
