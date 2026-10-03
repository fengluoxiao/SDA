"""Reject an empty GL viewport in a simulator screenshot (stdlib-only PNG)."""
import collections,json,pathlib,struct,sys,zlib

def read_png(path):
 data=pathlib.Path(path).read_bytes()
 if data[:8]!=b"\x89PNG\r\n\x1a\n":raise ValueError("Not a PNG")
 offset=8;compressed=bytearray()
 while offset<len(data):
  length=struct.unpack_from(">I",data,offset)[0];kind=data[offset+4:offset+8];chunk=data[offset+8:offset+8+length];offset+=length+12
  if kind==b"IHDR":width,height,depth,color,_,_,interlace=struct.unpack(">IIBBBBB",chunk)
  elif kind==b"IDAT":compressed.extend(chunk)
  elif kind==b"IEND":break
 if depth!=8 or color not in (2,6) or interlace!=0:raise ValueError("Unsupported PNG encoding")
 channels=3 if color==2 else 4;stride=width*channels;raw=zlib.decompress(compressed)
 previous=bytearray(stride);rows=[];offset=0
 for _ in range(height):
  mode=raw[offset];offset+=1;row=bytearray(raw[offset:offset+stride]);offset+=stride
  for i in range(stride):
   a=row[i-channels] if i>=channels else 0;b=previous[i];c=previous[i-channels] if i>=channels else 0
   if mode==1:predict=a
   elif mode==2:predict=b
   elif mode==3:predict=(a+b)//2
   elif mode==4:
    p=a+b-c;pa,pb,pc=abs(p-a),abs(p-b),abs(p-c)
    predict=a if pa<=pb and pa<=pc else b if pb<=pc else c
   elif mode==0:predict=0
   else:raise ValueError("Unknown PNG filter")
   row[i]=(row[i]+predict)&255
  rows.append(row);previous=row
 return width,height,channels,rows

def check(path,report_path):
 report=json.loads(pathlib.Path(report_path).read_text());bounds=report['bounds']
 width,height,channels,rows=read_png(path)
 # UIKit points -> screenshot pixels. Exclude rounded corners and adjacent labels.
 scale=width/report['screenWidth']
 left,top,right,bottom=[round(v*scale) for v in (bounds['x']+12,bounds['y']+12,bounds['x']+bounds['width']-12,bounds['y']+bounds['height']-12)]
 if not (0<=left<right<=width and 0<=top<bottom<=height):raise RuntimeError('Scene viewport outside screenshot: '+str(bounds))
 colors=collections.Counter(tuple(rows[y][x*channels:x*channels+3]) for y in range(top,bottom,2) for x in range(left,right,2))
 background=colors.most_common(1)[0][0]
 different=sum(n for rgb,n in colors.items() if max(abs(a-b) for a,b in zip(rgb,background))>12)
 summary={'background':background,'nonBackgroundSamples':different,'samples':sum(colors.values()),'crop':[left,top,right,bottom]}
 pathlib.Path(report_path).with_name('scene-image-check.json').write_text(json.dumps(summary,indent=2))
 if different<100:raise RuntimeError('Scene screenshot has no visible geometry: '+str(summary))
 print('Visible scene pixels verified:',summary)

if __name__=='__main__':check(sys.argv[1],sys.argv[2])
