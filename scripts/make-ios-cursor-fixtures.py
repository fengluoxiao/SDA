"""Synthetic container-only regression; no user music is read or copied."""
from pathlib import Path
import struct,json,sys
u32=lambda n:struct.pack('>I',n)
def box(kind,body):return u32(len(body)+8)+kind+body
def full(kind,body):return box(kind,bytes(4)+body)
def children(data):
 at=0
 while at<len(data):
  size,kind=struct.unpack_from('>I4s',data,at);header=8
  if size==1:size=struct.unpack_from('>Q',data,at+8)[0];header=16
  if size==0:size=len(data)-at
  assert size>=header and at+size<=len(data)
  yield kind,data[at+header:at+size];at+=size
 assert at==len(data)
def find(data,kind):return next(body for k,body in children(data) if k==kind)
def patch_clock(data,duration):
 d=bytearray(data);version=d[0];pos=20 if version else 12;struct.pack_into('>I',d,pos,48000);struct.pack_into('>Q' if version else '>I',d,pos+4,duration);return bytes(d)
def generate(source,out):
 raw=source.read_bytes();moov=find(raw,b'moov');ftyp=box(b'ftyp',find(raw,b'ftyp'))
 track=next(t for k,t in children(moov) if k==b'trak' and b'ec-3' in t);mdia=find(track,b'mdia');minf=find(mdia,b'minf');stbl=find(minf,b'stbl');stsd=find(stbl,b'stsd');sz=find(stbl,b'stsz');sc=find(stbl,b'stsc');co=find(stbl,b'stco')
 fixed,n=struct.unpack_from('>II',sz,4);sizes=[fixed]*n if fixed else list(struct.unpack_from('>'+str(n)+'I',sz,12));runs=[struct.unpack_from('>III',sc,8+i*12) for i in range(struct.unpack_from('>I',sc,4)[0])];offsets=struct.unpack_from('>'+str(struct.unpack_from('>I',co,4)[0])+'I',co,8)
 packets=[];r=0
 for chunk,offset in enumerate(offsets,1):
  while r+1<len(runs) and runs[r+1][0]<=chunk:r+=1
  for _ in range(min(runs[r][1],n-len(packets))):
   length=sizes[len(packets)];packets.append(raw[offset:offset+length]);offset+=length
 assert len(packets)==n and n>3
 # Match the failing metadata shape, using only the existing generated tones.
 count=9270;sequence=[packets[i%len(packets)] for i in range(count)];lengths=[len(v) for v in sequence];declared=count*1536-766
 payload=b''.join(sequence);out.mkdir(parents=True,exist_ok=True);(out/'expected.eac3').write_bytes(payload)
 def make(offsets,use64):
  table=box(b'stsd',stsd)+full(b'stts',u32(1)+u32(count)+u32(1536))+full(b'stsc',u32(2)+u32(1)+u32(15)+u32(1)+u32(618)+u32(15)+u32(1))
  table+=full(b'stsz',u32(0)+u32(count)+b''.join(u32(v) for v in lengths))
  table+=full(b'co64' if use64 else b'stco',u32(len(offsets))+b''.join(struct.pack('>Q' if use64 else '>I',v) for v in offsets))
  new_minf=b''.join(box(k,table if k==b'stbl' else v) for k,v in children(minf))
  new_mdia=b''.join(box(k,patch_clock(v,declared) if k==b'mdhd' else new_minf if k==b'minf' else v) for k,v in children(mdia))
  edit=full(b'elst',u32(1)+u32(declared)+struct.pack('>i',3072)+u32(0x10000))
  track_parts=[]
  for k,v in children(track):
   if k==b'edts':continue
   if k==b'mdia':v=new_mdia
   if k==b'tkhd':
    v=bytearray(v);struct.pack_into('>Q' if v[0] else '>I',v,28 if v[0] else 20,declared);v=bytes(v)
   track_parts.append(box(k,v))
  new_track=b''.join(track_parts)+box(b'edts',edit)
  return box(b'moov',b''.join(box(k,patch_clock(v,declared)) for k,v in children(moov) if k==b'mvhd')+box(b'trak',new_track))
 for use64,name in [(False,'cursor32.m4a'),(True,'cursor64.m4a')]:
  template=make([0]*618,use64);start=len(ftyp)+len(template)+8;positions=[];offset=start
  for i,v in enumerate(sequence):
   if i%15==0:positions.append(offset)
   offset+=len(v)
  built=make(positions,use64);assert len(built)==len(template);data=ftyp+built+box(b'mdat',payload);(out/name).write_bytes(data)
  if not use64:(out/'truncated.m4a').write_bytes(data[:-1])
 (out/'manifest.json').write_text(json.dumps({'samples':count,'sizes':lengths,'primingSamples':2,'editOvershootSamples':2306,'authoredSilenceOrToneFixtureOnly':True},indent=2))
if __name__=='__main__':generate(Path(sys.argv[1]),Path(sys.argv[2]))
