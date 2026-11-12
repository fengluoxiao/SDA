import {test} from 'node:test';
import assert from 'node:assert/strict';
import {build} from 'esbuild';
import {readFileSync} from 'node:fs';
const result=await build({entryPoints:['apps/mobile/src/ios-platform-version.ts'],bundle:true,platform:'node',format:'esm',write:false});
const {iosMajorVersion,supportsIOSGlass}=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].text).toString('base64'));
for(const [version,major]of[[26,26],['26',26],['26.1',26],['26.2.1',26],['27.0.1',27],[' 27.1.2 ',27],['25.7.1',25],['',0],['unknown',0],[NaN,0],[Infinity,0],[undefined,0],['26abc',0]])test('iOS capability parses '+String(version)+' without losing patch versions',()=>{assert.equal(iosMajorVersion(version),major);assert.equal(supportsIOSGlass('ios',version),major>=26);assert.equal(supportsIOSGlass('android',version),false);});
const bundles={};for(const file of['IOSMetadataSheet','IOSNativeSettings'])bundles[file]=(await build({entryPoints:['apps/mobile/src/'+file+'.tsx'],bundle:true,platform:'node',format:'cjs',write:false,packages:'external',plugins:[{name:'isolate-native-controls',setup(b){b.onResolve({filter:/^\./},args=>args.kind==='entry-point'||args.path==='./ios-platform-version'?undefined:{path:args.path,external:true});}}]})).outputFiles[0].text;
function render(file,version,run){
 const previous=globalThis.expo;globalThis.expo={getViewConfig:()=>({})};
 try{const module={exports:{}},React={createElement:(type,props,...children)=>({type,props:{...props,children}}),useState:()=>[false,()=>{}],useEffect(){}};
 const native={Platform:{OS:'ios',Version:version},View:'RNView',Text:'RNText',Modal:'RNModal',Pressable:'Pressable',PlatformColor:x=>x,StyleSheet:{create:x=>x},Alert:{alert(){}}};
 const ui={Host:'SwiftUIHost',Button:'SwiftUIButton',Text:'SwiftUIText',Image:'SwiftUIImage',Menu:'SwiftUIMenu',Picker:'SwiftUIPicker'};
 const modifiers=new Proxy({},{get:(_,name)=>(...args)=>({name,args})});
 new Function('require','module','exports',bundles[file])(name=>{
  if(name==='react')return React;if(name==='react-native')return native;if(name==='@expo/ui/swift-ui/modifiers')return modifiers;if(name==='./IOSSystemTabs')return{swiftUI:ui};if(name==='./IOSNativePage')return{hasNativePageHeader:false,IOSNativePage:'NativePage'};
  if(name==='./spatial-head-indicator')return{spatialHeadIndicator:()=>({label:'空间音频',symbol:'person.crop.circle',poseDegrees:0,active:false,color:'#7799CC'})};
  if(name==='./app-theme')return{CHARACTER_THEMES:[],useAppTheme:()=>({accent:'#7799CC'}),validTheme:()=>true};if(name==='./navigation-preferences')return{};if(name.endsWith('.json'))return{};return{};
 },module,module.exports);return run(module.exports);
 }finally{globalThis.expo=previous;}
}
const modifier=(node,name)=>node.props.modifiers.find(m=>m.name===name)?.args[0];
for(const version of['26.2.1','27.0.1']){
 test('Done text and capsule use one native layout on iOS '+version,()=>render('IOSMetadataSheet',version,api=>{let closed=0;const host=api.IOSMetadataDoneButton({onPress:()=>closed++,theme:'light',accent:'#7799CC'});assert.equal(host.type,'SwiftUIHost');assert.equal(host.props.ignoreSafeArea,'container');assert.equal(host.props.alignment,'center');const button=host.props.children[0],label=button.props.children[0];assert.equal(button.type,'SwiftUIButton');assert.equal(modifier(button,'buttonStyle'),'glass');assert.equal(modifier(button,'frame').alignment,'center');assert.equal(label.type,'SwiftUIText');assert.deepEqual(label.props.children,['完成']);assert.equal(modifier(label,'foregroundColor'),'#7799CC');assert.equal(modifier(label,'frame').alignment,'center');assert.equal(modifier(label,'opacity'),undefined);button.props.onPress();assert.equal(closed,1);assert(!JSON.stringify(host).includes('RNText'));}));
 test('both header actions retain native Liquid Glass on iOS '+version,()=>render('IOSNativeSettings',version,api=>{let opened=0;const settings=api.IOSSettingsButton({onPress:()=>opened++}),head=api.IOSHeadTrackingButton({enabled:false,disabled:false,accent:'#7799CC',onSelect(){}});for(const host of[settings,head]){assert.equal(host.props.ignoreSafeArea,'container');assert.equal(host.props.alignment,'center');const control=host.props.children[0];assert.equal(modifier(control,'buttonStyle'),'glass');assert.equal(modifier(control,'buttonBorderShape'),'circle');assert.deepEqual(host.props.style.width,44);}settings.props.children[0].props.onPress();assert.equal(opened,1);}));
}
test('older iOS keeps its supported native bordered control',()=>render('IOSMetadataSheet','25.7.1',api=>{const host=api.IOSMetadataDoneButton({onPress(){},theme:'light',accent:'#7799CC'});assert.equal(modifier(host.props.children[0],'buttonStyle'),'bordered');}));
test('glass capability gates share the version parser instead of Number on dotted strings',()=>{for(const file of['IOSMetadataSheet','IOSMiniPlayerBackdrop','IOSNativeSettings','IOSUIKitTabs','IOSVerticalTabBar','SideNavigationRail']){const text=readFileSync('apps/mobile/src/'+file+'.tsx','utf8');assert(!text.includes('Number(Platform.Version)'));assert(text.includes('supportsIOSGlass'));}const player=readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8');assert.match(player,/standaloneMiniGlass = sideNavigationActive \|\| supportsIOSGlass/);assert.match(player,/!environment && standaloneMiniGlass && <IOSMiniPlayerBackdrop/);});

test('the shared native Done button supports AI drawer accessibility and blocks saving-time presses',()=>render('IOSMetadataSheet','26.2.1',api=>{
 let closed=0;const host=api.IOSMetadataDoneButton({onPress:()=>closed++,theme:'dark',accent:'#7799CC',accessibilityLabel:'关闭 AI 场景设置',disabled:true});
 const button=host.props.children[0];assert.equal(modifier(button,'accessibilityLabel'),'关闭 AI 场景设置');assert.equal(modifier(button,'disabled'),true);
 button.props.onPress();assert.equal(closed,0);
}));
