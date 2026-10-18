import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {inflateSync} from 'node:zlib';
import {build} from 'esbuild';
const source=readFileSync('apps/mobile/src/IOSVerticalTabBar.tsx','utf8');
assert.doesNotMatch(source,/vertical-tab-upright-symbols|GestureDetector|onTouchMove|sideNavigationAppearance|setReportedInsets|SafeAreaListener/,'native items own glyph placement and highlight');
const bundle=await build({entryPoints:['apps/mobile/src/IOSVerticalTabBar.tsx'],bundle:true,platform:'node',format:'cjs',write:false,define:{__DEV__:'false'},external:['react','react-native','react-native-screens','react-native-safe-area-context','*.png']});
function load({os='ios',isPad=false,version=26,available=true,insets={left:59,right:59,bottom:21},reported=null}={}) {
 const mod={exports:{}},updates=[];let slot=0;
 new Function('require','module','exports',bundle.outputFiles[0].text)(name=>{
  if(name==='react')return {useRef:value=>({current:value}),createElement:(type,props,...children)=>({type,props:{...props,children}}),useState:initial=>{const index=slot++;return[index===1?reported:initial,value=>updates.push([index,value])];}};
  if(name==='react-native')return {AppState:{currentState:'active'},Platform:{OS:os,Version:version,isPad},UIManager:{hasViewManagerConfig:()=>available},View:'View'};
  if(name==='react-native-safe-area-context')return {useSafeAreaInsets:()=>insets,SafeAreaListener:'SafeAreaListener'};
  if(name==='react-native-screens')return {Tabs:{Host:'NativeTabsHost',Screen:'NativeTabsScreen'}};
  if(name.endsWith('.png'))return name;
  throw Error(name);
 },mod,mod.exports);
 return {...mod.exports,updates};
}
for(const accent of ['#7799CC','#FFEE55','#EE5577','#9977CC','#BB9955'])for(const dark of [false,true])for(const selected of [0,1,2]) {
 const api=load(),events=[];
 const tree=api.IOSVerticalTabBar({selected,onChange:index=>events.push(index),accent,dark});
 assert.equal(tree.props.children.length,1,'no external symbol overlay');
 const rotation=tree.props.children[0],compensation=rotation.props.children[0],native=compensation.props.children[0];
 assert.equal(rotation.props.style.transform[0].rotate,'90deg');
 assert.equal(tree.props.style.height,140);
 assert.equal(rotation.props.style.width,140);
 assert.equal(rotation.props.style.left+rotation.props.style.width/2-24,0);
 assert.equal(rotation.props.style.top+24-rotation.props.style.width/2,0);
 assert.equal(compensation.props.style.width,216);assert.equal(compensation.props.style.left,-38);
 assert.equal(native.type,'NativeTabsHost');assert.deepEqual(native.props.style,{width:216,height:69,flex:0});assert.equal(native.props.navStateRequest.selectedScreenKey,String(selected));
 assert.equal(native.props.ios.tabBarTintColor,accent);assert.equal(native.props.colorScheme,dark?'dark':'light');
 const items=native.props.children[0];assert.equal(items.length,3);
 const images=items.map(item=>item.props.ios.icon.templateSource);assert.equal(new Set(images).size,3);
 assert(images.every(image=>/side-tabs\/(play|library|space)-ccw.png$/.test(image)));
 for(const item of items) {
  assert.equal(item.props.title,undefined,'icon-only native items must not reserve an invisible title');
  assert.equal(item.props.ios.icon.type,'templateSource');assert.equal(item.props.ios.standardAppearance,item.props.ios.scrollEdgeAppearance);
  assert.deepEqual(item.props.ios.standardAppearance,{tabBarBackgroundColor:'transparent',tabBarShadowColor:'transparent',tabBarBlurEffect:'systemDefault'});
 }
 assert.deepEqual(items.map(item=>item.props.tabBarItemAccessibilityLabel),['播放','资料库','空间']);
 for(const key of ['', '9',String(selected)])native.props.onTabSelected({nativeEvent:{selectedScreenKey:key,provenance:1,actionOrigin:"user"}});
 assert.deepEqual(events,[]);
 native.props.onTabSelected({nativeEvent:{selectedScreenKey:String((selected+1)%3),provenance:7,actionOrigin:"user"}});
 assert.deepEqual(events,[(selected+1)%3]);
 for(const actionOrigin of ['implicit','programmatic-js','programmatic-native'])native.props.onTabSelected({nativeEvent:{selectedScreenKey:String((selected+2)%3),provenance:8,actionOrigin}});
 assert.deepEqual(events,[(selected+1)%3],'mount/sync callbacks cannot change the actual page');
 assert.equal(tree.props.onTouchMove,undefined);
}
for(const options of [{os:'android'},{version:25},{available:false}])assert.equal(load(options).hasNativeVerticalTabBar,false);
assert.match(readFileSync('apps/mobile/src/IOSUIKitTabs.tsx','utf8'),/const appearance = iosTabAppearance/);

// Decode these generated RGBA PNGs with only Node built-ins. An all-transparent
// placeholder would pass layout tests but leave the real tab items invisible.
function png(path) {
 const data=Buffer.isBuffer(path)?path:readFileSync(path),chunks=[];let w,h;
 for(let p=8;p<data.length;) {const len=data.readUInt32BE(p),type=data.toString('ascii',p+4,p+8),chunk=data.subarray(p+8,p+8+len);p+=12+len;
  if(type==='IHDR'){w=chunk.readUInt32BE(0);h=chunk.readUInt32BE(4);assert.equal(chunk[8],8);assert.equal(chunk[9],6);}
  if(type==='IDAT')chunks.push(chunk);
 }
 const raw=inflateSync(Buffer.concat(chunks)),pixels=Buffer.alloc(w*h*4),stride=w*4;
 const paeth=(a,b,c)=>{const p=a+b-c,pa=Math.abs(p-a),pb=Math.abs(p-b),pc=Math.abs(p-c);return pa<=pb&&pa<=pc?a:pb<=pc?b:c;};
 for(let y=0;y<h;y++){const filter=raw[y*(stride+1)];for(let x=0;x<stride;x++){
  const i=y*stride+x,a=x>=4?pixels[i-4]:0,b=y>0?pixels[i-stride]:0,c=x>=4&&y>0?pixels[i-stride-4]:0;
  const predictor=[0,a,b,Math.floor((a+b)/2),paeth(a,b,c)][filter];assert.notEqual(predictor,undefined);
  pixels[i]=(raw[y*(stride+1)+1+x]+predictor)&255;
 }}
 return {w,h,pixels};
}
for(const name of ['play','library','space']) {
 const upright=png(`apps/mobile/assets/side-tabs/sources/${name}.png`);
 for(const [suffix,scale] of [['',1],['@2x',2],['@3x',3]]) {
  const icon=png(`apps/mobile/assets/side-tabs/${name}-ccw${suffix}.png`);assert.equal(icon.w,44*scale);assert.equal(icon.h,24*scale);
  const alphas=[...icon.pixels].filter((_,i)=>i%4===3);assert(alphas.some(a=>a===255));assert(alphas.some(a=>a===0));
  for(let y=0;y<icon.h;y++)for(let x=0;x<icon.w;x++)if(x<7*scale||x>=31*scale)assert.equal(icon.pixels[(y*icon.w+x)*4+3],0,'added spacing is transparent');
  if(scale===3)for(let y=0;y<72;y++)for(let x=0;x<72;x++)assert.equal(icon.pixels[((71-x)*132+y+21)*4+3],upright.pixels[(y*72+x)*4+3],'padded native items preserve exact glyph size, inverse rotation and the 3-point optical offset');
 }
}
console.log('Native tab item icons: real opaque alpha glyphs, inverse pixel rotation at all display scales, system tint and native selection ownership passed. Physical layout still needs device review.');

// Parent safe insets own controller sizing. Child page layout must never reset
// the 140-point visible bar length after its first layout.
for(const left of [0,44,59,62])for(const right of [0,44,59,62])for(const bottom of [0,21,34]) {
 const api=load({insets:{left,right,bottom}}),tree=api.IOSVerticalTabBar({selected:0,onChange(){},accent:'#7799CC',dark:false});
 const compensation=tree.props.children[0].props.children[0],host=compensation.props.children[0];
 assert.equal(host.props.style.width-Math.max(0,left-bottom)-Math.max(0,right-bottom),140);
 assert.equal(compensation.props.style.left,-Math.max(0,left-bottom));assert.equal(host.props.style.height-bottom,48);
 for(const page of host.props.children[0])assert.equal(page.props.children[0].props.onChange,undefined);
}

// Tablet fitting space remains wide while its visible capsule docks in a compact frame.
for(const bottom of [0,20,34]) {
 const tree=load({isPad:true,insets:{left:0,right:0,bottom}}).IOSVerticalTabBar({selected:0,onChange(){},accent:'#7799CC',dark:false});
 const rotation=tree.props.children[0],host=rotation.props.children[0].props.children[0];
 assert.equal(tree.props.style.height,160);assert.equal(rotation.props.style.width,320);
 assert.equal(rotation.props.style.left,(48-320)/2);assert.equal(rotation.props.style.top,(160-48)/2);
 assert.equal(rotation.props.style.top+24,tree.props.style.height/2,"native capsule center follows the visible dock, not blank controller space");
 assert.equal(host.props.style.width,320);assert.equal(host.props.style.height,48+bottom);
 assert.equal(host.props.children[0].length,3);
}

// iPad templates have no wide transparent phone canvas and no network URI.
for(const dark of [false,true])for(const selected of [0,1,2]) {
 const events=[];
 const tree=load({isPad:true}).IOSVerticalTabBar({selected,onChange:index=>events.push(index),accent:'#EE5577',dark});
 const host=tree.props.children[0].props.children[0].props.children[0];
 assert.equal(host.props.ios.tabBarTintColor,'#EE5577');
 const items=host.props.children[0];
 for(const [index,name] of ['play','library','space'].entries()) {
  const icon=items[index].props.ios.icon;
  assert.equal(icon.type,'templateSource');
  assert.match(icon.templateSource.uri,/^data:image\/png;base64,/);
  assert.equal(icon.templateSource.width,24);assert.equal(icon.templateSource.height,24);assert.equal(icon.templateSource.scale,3);
  const image=png(Buffer.from(icon.templateSource.uri.split(',')[1],'base64'));
  const upright=png(`apps/mobile/assets/side-tabs/sources/${name}.png`);
  assert.equal(image.w,72);assert.equal(image.h,72);
  for(let y=0;y<72;y++)for(let x=0;x<72;x++)assert.equal(image.pixels[((71-x)*72+y)*4+3],upright.pixels[(y*72+x)*4+3],'tablet glyph preserves silhouette and inverse rotation without wide padding');
 }
 host.props.onTabSelected({nativeEvent:{selectedScreenKey:String((selected+1)%3),provenance:7,actionOrigin:'user'}});
 assert.deepEqual(events,[(selected+1)%3]);
}
console.log('iPad icons: square 24pt alpha templates, inverse rotation, inline loading, theme tint and native selection passed.');
