// Length-prefixed decoder frames. Each incoming byte is copied once rather
// than concatenating the entire partially received multichannel PCM frame.
class DecoderFrameReader {
 constructor(onFrame){this.onFrame=onFrame;this.prefix=Buffer.allocUnsafe(4);this.prefixAt=0;this.metadata=null;this.metadataAt=0;this.header=null;this.payload=null;this.payloadAt=0;}
 push(bytes){
  let at=0;
  while(at<bytes.length){
   if(!this.metadata){
    const n=Math.min(4-this.prefixAt,bytes.length-at);bytes.copy(this.prefix,this.prefixAt,at,at+n);this.prefixAt+=n;at+=n;
    if(this.prefixAt<4)return;
    const size=this.prefix.readUInt32LE();if(size<2||size>32*1024*1024)throw Error('SDAR 解码协议不匹配');
    this.metadata=Buffer.allocUnsafe(size);this.metadataAt=0;
   }
   if(!this.header){
    const n=Math.min(this.metadata.length-this.metadataAt,bytes.length-at);bytes.copy(this.metadata,this.metadataAt,at,at+n);this.metadataAt+=n;at+=n;
    if(this.metadataAt<this.metadata.length)return;
    this.header=JSON.parse(this.metadata.toString('utf8'));
    if(!this.header||!Number.isSafeInteger(this.header.dataBytes)||this.header.dataBytes<0||this.header.dataBytes>2*1024*1024)throw Error('SDAR 解码数据长度错误');
    this.payload=Buffer.allocUnsafe(this.header.dataBytes);this.payloadAt=0;
   }
   const n=Math.min(this.payload.length-this.payloadAt,bytes.length-at);bytes.copy(this.payload,this.payloadAt,at,at+n);this.payloadAt+=n;at+=n;
   if(this.payloadAt<this.payload.length)return;
   const header=this.header,payload=this.payload;
   this.prefixAt=0;this.metadata=null;this.header=null;this.payload=null;
   this.onFrame(header,payload);
  }
 }
}
module.exports={DecoderFrameReader};
