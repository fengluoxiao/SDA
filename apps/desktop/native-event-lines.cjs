'use strict';
// Output metadata may contain all 128 object states; control packets have a separate limit.
function readNativeEventLines(previous,chunk,maxBytes=1024*1024){
 const value=previous+chunk,lines=[];let start=0,overflow=false;
 for(;;){const end=value.indexOf('\n',start);if(end<0)break;const line=value.slice(start,end);start=end+1;if(Buffer.byteLength(line,'utf8')>maxBytes){overflow=true;continue;}if(line)lines.push(line);}
 let remaining=value.slice(start);if(Buffer.byteLength(remaining,'utf8')>maxBytes){remaining='';overflow=true;}
 return{lines,remaining,overflow};
}
module.exports={readNativeEventLines};
