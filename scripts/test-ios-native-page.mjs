import assert from 'node:assert/strict';
import {build} from 'esbuild';
const built=await build({entryPoints:['apps/mobile/src/IOSNativePage.tsx'],bundle:true,platform:'node',format:'cjs',write:false,external:['react','react-native','react-native-screens']});
function load(platform,available){
 const m={exports:{}};let imports=0;
 const react={createElement:(type,props,...children)=>({type,props:{...props,children}}),Fragment:'Fragment'};
 new Function('require','module','exports','__DEV__',built.outputFiles[0].text)(name=>{
  if(name==='react')return react;
  if(name==='react-native')return {Platform:{OS:platform},PlatformColor:name=>name,StyleSheet:{absoluteFill:{position:'absolute'}},View:'View',UIManager:{hasViewManagerConfig:()=>{if(available==='throws')throw Error('missing');return available;}}};
  if(name==='react-native-screens'){imports++;return {ScreenStack:'Stack',ScreenStackItem:'Screen',ScreenStackHeaderLeftView:'HeaderLeft',ScreenStackHeaderRightView:'HeaderRight'};}
  throw Error(name);
 },m,m.exports,false);
 return {...m.exports,imports};
}
for(const title of ['设置','资料库']) for(const theme of ['light','dark']) {
 const api=load('ios',true);const content={type:'ScrollView',props:{}};
 const heights=[];const root=api.IOSNativePage({screenId:'page',title,theme,accent:'#7799CC',headerLeft:'back',headerRight:'actions',onHeaderHeightChange:h=>heights.push(h),children:content});
 assert.equal(root.type,'Stack');const screen=root.props.children[0];
 assert.equal(screen.type,'Screen');assert.equal(screen.props.children[0],content);
 screen.props.onHeaderHeightChange({nativeEvent:{headerHeight:78}});assert.deepEqual(heights,[78]);
 const h=screen.props.headerConfig;assert.equal(h.title,title);assert.equal(h.largeTitle,false);assert.equal(h.translucent,true);assert.equal(h.backgroundColor,'transparent');
 assert.equal(h.blurEffect,undefined);assert.equal(screen.props.scrollEdgeEffects,undefined);assert.equal(screen.props.stackAnimation,undefined);
 assert.equal(h.children.props.children[0].type,'HeaderLeft');assert.equal(h.children.props.children[1].type,'HeaderRight');
 assert.equal(h.children.props.children[1].props.children[0].props.children[0],'actions');
}
for(const [platform,available] of [['ios',false],['ios','throws'],['android',true]]) {
 const api=load(platform,available);assert.equal(api.hasNativePageHeader,false);assert.equal(api.imports,0);
 const fallback=api.IOSNativePage({screenId:'page',title:'设置',theme:'light',accent:'#7799CC',children:'content'});assert.equal(fallback.props.children[0],'content');
}
console.log('Native pages: UIKit-owned headers, preserved actions/content, native effects and safe capability fallbacks passed.');
