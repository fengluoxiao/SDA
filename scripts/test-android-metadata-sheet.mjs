import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {transformSync} from 'esbuild';
const source=readFileSync('apps/mobile/src/AndroidMetadataSheet.tsx','utf8');
const layoutApi={exports:{}};
new Function('module','exports',transformSync(readFileSync('apps/mobile/src/android-metadata-layout.ts','utf8'),{loader:'ts',format:'cjs'}).code)(layoutApi,layoutApi.exports);
const {androidMetadataBodyHeight}=layoutApi.exports;
for(const [height,top,bottom] of [[800,24,16],[360,0,24],[280,44,24],[120,30,20],[0,0,0]]){
 const value=androidMetadataBodyHeight(height,top,bottom);assert(value>=0&&value<=height);
 if(value>0)assert(value+28+Math.max(16,top)+12+Math.max(16,bottom)<=height);
}
assert.equal(androidMetadataBodyHeight(NaN,0,0),0);
let hideResolve,hides=0,slot=0,bounds=null;
const api={exports:{}},events=[];
const react={createElement:(type,props,...children)=>({type,props:{...props,children}}),isValidElement:x=>!!x?.props,
 cloneElement:(x,p)=>({...x,props:{...x.props,...p}}),useRef:value=>slot++===0?{current:{hide:()=>{hides++;return new Promise(resolve=>{hideResolve=resolve;});}}}:{current:value},
 useState:()=>[bounds,next=>{bounds=typeof next==='function'?next(bounds):next}],useCallback:fn=>fn};
const compose={Host:'Host',ModalBottomSheet:Object.assign(function ModalBottomSheet(){},{DragHandle:'DragHandle'}),Box:'Box',RNHostView:'RNHostView'};
const modifiers=Object.fromEntries(['fillMaxWidth','height','size','clip','background','onSizeChanged'].map(name=>[name,(...args)=>({name,args})]));
const old=globalThis.expo;globalThis.expo={modules:{ExpoUI:{}}};
new Function('require','module','exports',transformSync(source,{loader:'tsx',format:'cjs'}).code)(name=>{
 const imports={react,'react-native':{Platform:{OS:'android'},Modal:'Modal',View:'View',useWindowDimensions:()=>({width:390,height:800})},'react-native-safe-area-context':{useSafeAreaInsets:()=>({top:24,bottom:16,left:0,right:0})},'./android-metadata-layout':layoutApi.exports,'./app-theme':{useAppTheme:()=>({id:'sakiko',accent:'#7799CC'})},'./android-material-theme':{androidMaterialTheme:()=>({bg:'#f2f2f7'})},'@expo/ui/jetpack-compose':compose,'@expo/ui/jetpack-compose/modifiers':modifiers};
 assert(name in imports,name);return imports[name];
},api,api.exports);globalThis.expo=old;
const content={type:'Metadata',props:{onClose:()=>assert.fail('sheet owns the animated close')}};
const tree=api.exports.AndroidMetadataSheet({open:true,onChange:open=>events.push(open),theme:'light',children:content});
const sheet=tree.props.children[0],handle=sheet.props.children[0],measuredBody=sheet.props.children[1];
assert.equal(sheet.type,compose.ModalBottomSheet);assert.equal(sheet.props.skipPartiallyExpanded,false);assert.equal(handle.type,compose.ModalBottomSheet.DragHandle);
const initial=measuredBody.type(measuredBody.props);assert.equal(initial.type,'Box');assert(!initial.props.children[0],'do not mount RN content before the native viewport exists');
const measured=initial.props.modifiers.find(m=>m.name==='onSizeChanged').args[0];
measured({width:0,height:0});assert.equal(bounds,null);measured({width:Infinity,height:500});assert.equal(bounds,null);
measured({width:390,height:720});let body=measuredBody.type(measuredBody.props),host=body.props.children[0],view=host.props.children[0],metadata=view.props.children.flat(Infinity)[0];
assert.equal(host.type,'RNHostView');assert.equal(host.props.matchContents,true);assert.deepEqual(view.props.style,{width:390,height:720});assert.equal(metadata.props.sheet,true);
measured({width:640,height:280});body=measuredBody.type(measuredBody.props);assert.deepEqual(body.props.children[0].props.children[0].props.style,{width:640,height:280},'use real landscape/dialog bounds rather than screen guesses');
const closed=metadata.props.onClose();await metadata.props.onClose();assert.equal(hides,1);assert.deepEqual(events,[]);hideResolve();await closed;assert.deepEqual(events,[false]);sheet.props.onDismissRequest();assert.deepEqual(events,[false,false]);
assert.equal(api.exports.AndroidMetadataSheet({open:false,onChange(){},theme:'light',children:content}),null,'closing unmounts the measured body so reopening starts with fresh native bounds');
console.log('Android metadata: measured first presentation, invalid-size guard, native width/height updates, safe viewport and animated dismissal passed.');

const audioSource=readFileSync('apps/mobile/src/AndroidAudioInfo.tsx','utf8'),audio={exports:{}};
new Function('require','module','exports',transformSync(audioSource,{loader:'tsx',format:'cjs'}).code)(name=>{
 const imports={react,'react-native':{ScrollView:'ScrollView',StatusBar:'StatusBar',Text:'Text',View:'View',StyleSheet:{create:value=>value}},'react-native-safe-area-context':{SafeAreaView:'SafeAreaView'},'./AndroidChrome':{AndroidGlassAction:'Done',hasAndroidChrome:true},'./AndroidGlassSettingsButton':{AndroidGlassSettingsButton:'Back'},'./app-theme':{useAppTheme:()=>({id:'sakiko'})},'./android-material-theme':{androidMaterialTheme:()=>({bg:'#fff',ink:'#000',muted:'#444',primary:'#123'})}};
 assert(name in imports,name);return imports[name];
},audio,audio.exports);
const props={rows:[['文件名','first.m4a']],dark:false,onClose(){},sheet:true};
const first=audio.exports.AndroidAudioInfo(props),updated=audio.exports.AndroidAudioInfo({...props,rows:[['文件名','first.m4a'],['编码','SDAR']]});
const scroll=first.props.children[2],nextScroll=updated.props.children[2];
assert.equal(scroll.type,'ScrollView');assert(scroll.props.nestedScrollEnabled);assert.equal(scroll.props.style.flex,1);
assert.deepEqual(scroll.props.contentOffset,{x:0,y:0});assert.equal(scroll.props.contentOffset,nextScroll.props.contentOffset,'metadata updates must not send a fresh scroll-offset object');
assert.equal(first.props.children[1].props.style[0].flexShrink,0,'header remains above the list while viewport layout settles');
console.log('Metadata header and list: stable initial top offset, bounded nested scrolling and live metadata update behavior passed.');
