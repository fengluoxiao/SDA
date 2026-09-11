"use strict";
// Match the native JSON frame byte limit, including UTF-8 and punctuation.
// Avoid arbitrary event-count splits and repeatedly serializing the full prefix.
function eventBatches(events,maxBytes=16000){
 if(!Array.isArray(events)||events.length>4096)return null;
 const overhead=Buffer.byteLength(JSON.stringify({type:"objectEvents",events:[]}));
 const result=[];let batch=[],bytes=overhead;
 for(const event of events){
  let encoded;try{encoded=JSON.stringify(event);}catch{return null;}
  if(typeof encoded!=="string")return null;
  const size=Buffer.byteLength(encoded);
  if(size+overhead>maxBytes)return null;
  if(bytes+size+(batch.length?1:0)>maxBytes){result.push(batch);batch=[];bytes=overhead;}
  bytes+=size+(batch.length?1:0);batch.push(event);
 }
 if(batch.length)result.push(batch);
 return result;
}
module.exports={eventBatches};
