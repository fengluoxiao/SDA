import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {transformSync} from 'esbuild';
const source=readFileSync('apps/mobile/src/AndroidMetadataSheet.tsx','utf8');
let hideResolve, hides=0, slot=0;
const api={exports:{}},events=[];
const react={createElement:(type,props,...children)=>({type,props:{...props,children}}),isValidElement:x=>!!x?.props,
 cloneElement:(x,p)=>({...x,props:{...x.props,...p}}),useRef:value=>slot++===0?{current:{hide:()=>{hides++;return new Promise(resolve=>{hideResolve=resolve;});}}}:{current:value}};
const old=globalThis.expo;globalThis.expo={modules:{ExpoUI:{}}};
new Function('require','module','exports',transformSync(source,{loader:'tsx',format:'cjs'}).code)(name=>{
 const imports={react,'react-native':{Platform:{OS:'android'},Modal:'Modal',View:'View',useWindowDimensions:()=>({height:800})},'./app-theme':{useAppTheme:()=>({id:'sakiko',accent:'#7799CC'})},'./android-material-theme':{androidMaterialTheme:()=>({bg:'#f2f2f7'})},'@expo/ui/jetpack-compose':{Host:'Host',ModalBottomSheet:'ModalBottomSheet',RNHostView:'RNHostView'}};
 assert(name in imports,name);return imports[name];
},api,api.exports);globalThis.expo=old;
const content={type:'Metadata',props:{onClose:()=>assert.fail('sheet owns the animated close')}};
const tree=api.exports.AndroidMetadataSheet({open:true,onChange:open=>events.push(open),theme:'light',children:content});
const sheet=tree.props.children[0],host=sheet.props.children[0],body=host.props.children[0],metadata=body.props.children[0];
assert.equal(sheet.type,'ModalBottomSheet');assert.equal(sheet.props.skipPartiallyExpanded,false);assert.equal(host.props.matchContents,true);
assert.equal(body.props.style.height,700);assert.equal(metadata.props.sheet,true);
const closed=metadata.props.onClose();await metadata.props.onClose();assert.equal(hides,1);assert.deepEqual(events,[],'wait for native close animation');
hideResolve();await closed;assert.deepEqual(events,[false]);sheet.props.onDismissRequest();assert.deepEqual(events,[false,false]);
assert.match(source,/Platform.OS === "android"/,'iOS never requires Compose sheet native managers');
console.log('Android metadata: real partial/full native sheet, hosted content and single animated close/dismiss routing passed.');
