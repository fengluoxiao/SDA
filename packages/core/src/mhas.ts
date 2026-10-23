/** MHAS packet header, ISO/IEC 23008-3 escapedValue syntax. */
export function mhasPacket(type:number,payload:Uint8Array):Uint8Array {
  const bits:number[]=[];
  const put=(v:number,n:number)=>{for(let i=n-1;i>=0;i--)bits.push((v>>>i)&1);};
  const escaped=(v:number,a:number,b:number,c:number)=>{const max=2**a-1;put(Math.min(v,max),a);if(v>=max){v-=max;const m=2**b-1;put(Math.min(v,m),b);if(v>=m)put(v-m,c);}};
  escaped(type,3,8,8);escaped(1,2,8,32);escaped(payload.length,11,24,24);
  const result=new Uint8Array(bits.length/8+payload.length);
  bits.forEach((b,i)=>result[i>>3]!|=b<<(7-(i&7)));result.set(payload,bits.length/8);return result;
}
