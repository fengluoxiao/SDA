/** Experimental structural HRTF. NOT a measurement or reconstruction of a listener's anatomy.
 * First-order spherical head shadow + selectable ITD approximation + tunable pinna notch.
 * Pinna frequency/elevation mapping is a heuristic to be evaluated by the listener.
 */
export const MODEL_VERSION=1;
function validSingle(p){return p?.version===1&&["woodworth","low-frequency"].includes(p.itd)
 &&Number.isFinite(p.radius)&&p.radius>=.07&&p.radius<=.105
 &&Number.isFinite(p.notchHz)&&p.notchHz>=6500&&p.notchHz<=10000
 &&Number.isFinite(p.notchDb)&&p.notchDb>=3&&p.notchDb<=12;}
export function validParameters(p){
 if(validSingle(p))return true;
 return p?.version===2&&Number.isFinite(p.power)&&p.power>=1&&p.power<=4&&Array.isArray(p.anchors)&&p.anchors.length>0&&p.anchors.length<=64
  &&p.anchors.every(a=>typeof a.name==="string"&&a.name.length>0&&a.name.length<128&&Number.isFinite(a.az)&&Math.abs(a.az)<=180&&Number.isFinite(a.el)&&Math.abs(a.el)<=90&&validSingle(a.parameters))
  &&new Set(p.anchors.map(a=>a.name)).size===p.anchors.length;
}
export function parameterKey(p){if(!validParameters(p))throw new Error("Invalid pHRTF parameters");
 return p.version===2?JSON.stringify({version:2,power:p.power,anchors:p.anchors}):`${p.version}:${p.itd}:${p.radius}:${p.notchHz}:${p.notchDb}`;}
function resolveParameters(az,el,field){
 const vector=(a,e)=>{a*=Math.PI/180;e*=Math.PI/180;return [Math.sin(a)*Math.cos(e),Math.sin(e),Math.cos(a)*Math.cos(e)];};
 const v=vector(az,el),items=field.anchors.map(a=>{const b=vector(a.az,a.el);return {p:a.parameters,d:Math.hypot(...v.map((x,i)=>x-b[i]))};});
 const exact=items.find(a=>a.d<1e-8);if(exact)return {...exact.p,itdBlend:exact.p.itd==="woodworth"?0:1};
 let total=0,radius=0,notchHz=0,notchDb=0,itdBlend=0;
 for(const {p,d} of items){const w=1/d**field.power;total+=w;radius+=w*p.radius;notchHz+=w*p.notchHz;notchDb+=w*p.notchDb;itdBlend+=w*(p.itd==="woodworth"?0:1);}
 return {radius:radius/total,notchHz:notchHz/total,notchDb:notchDb/total,itdBlend:itdBlend/total};
}
/** A fresh independent model, sampled continuously. No profile, template or prior HRIR input. */
export function generateParameters(random=Math.random){
 const sample=()=>{const v=random();if(!Number.isFinite(v)||v<0||v>=1)throw new Error("Invalid random source");return v;};
 return {version:1,itd:sample()<.5?"woodworth":"low-frequency",radius:.07+.035*sample(),notchHz:6500+3500*sample(),notchDb:3+9*sample()};
}
export function synthesizeHrir(azimuth,elevation,p){
 if(!validParameters(p)||!Number.isFinite(azimuth)||!Number.isFinite(elevation)||Math.abs(elevation)>90)throw new Error("Invalid synthetic HRTF input");
 if(p.version===2)p=resolveParameters(azimuth,elevation,p);
 const sr=48000,n=512,out=new Float32Array(n*2),az=azimuth*Math.PI/180,el=elevation*Math.PI/180;
 const lateral=Math.sin(az)*Math.cos(el),theta=Math.asin(Math.min(1,Math.abs(lateral)));
 const blend=p.itdBlend??(p.itd==="woodworth"?0:1);
 const itd=p.radius/343*((1-blend)*(theta+Math.sin(theta))+blend*3*Math.abs(lateral));
 const rear=(1-Math.cos(az)*Math.cos(el))/2;
 const notch=Math.max(4500,Math.min(14000,p.notchHz+elevation/90*3000-rear*1400));
 // RBJ peaking equalizer used as a moderate notch. Same frequency mapping for both ears.
 const w=2*Math.PI*notch/sr,A=10**(-p.notchDb/40),alpha=Math.sin(w)/(2*3),a0=1+alpha/A;
 const b0=(1+alpha*A)/a0,b1=-2*Math.cos(w)/a0,b2=(1-alpha*A)/a0,a1=b1,a2=(1-alpha/A)/a0;
 for(let ear=0;ear<2;ear++){
  const side=ear===0?lateral:-lateral,incidence=Math.acos(Math.max(-1,Math.min(1,side)));
  const shadow=1.05+.95*Math.cos(incidence/(150*Math.PI/180)*Math.PI),beta=2*343/p.radius,k=2*sr;
  const hb0=(shadow*k+beta)/(k+beta),hb1=(beta-shadow*k)/(k+beta),ha1=(beta-k)/(beta+k);
  const samples=new Float64Array(n);let priorX=0,priorY=0,x1=0,x2=0,y1=0,y2=0;
  for(let i=0;i<n;i++){const x=i===0?1:0,h=hb0*x+hb1*priorX-ha1*priorY;priorX=x;priorY=h;
   const y=b0*h+b1*x1+b2*x2-a1*y1-a2*y2;x2=x1;x1=h;y2=y1;y1=y;samples[i]=y;}
  const delay=32+(side<0?itd*sr:0),integer=Math.floor(delay),fraction=delay-integer;
  // Fractional delay with a shared 16-sample windowed-sinc latency; no per-ear gain normalization.
  const kernel=[];let sum=0;
  for(let j=0;j<=32;j++){const x=j-16-fraction,v=(Math.abs(x)<1e-12?1:Math.sin(Math.PI*x)/(Math.PI*x))*(.5-.5*Math.cos(2*Math.PI*j/32));kernel.push(v);sum+=v;}
  for(let i=0;i<n-integer;i++)for(let j=0;j<=32&&i+integer+j<n;j++)out[ear*n+i+integer+j]+=samples[i]*kernel[j]/sum;
 }
 return out;
}
