import assert from 'node:assert/strict';
import {build} from 'esbuild';
const output=await build({entryPoints:['apps/mobile/src/side-navigation-gesture.ts'],bundle:true,platform:'node',format:'esm',write:false});
const {createSideRailGesture,sideRailIndex,sideRailIndicatorOffset}=await import('data:text/javascript;base64,'+Buffer.from(output.outputFiles[0].text).toString('base64'));
const p=y=>({x:24,y});
assert.deepEqual([24,70,116].map(y=>sideRailIndex(p(y))),[0,1,2]);
assert.equal(sideRailIndex({x:100,y:70}),null);
let g=createSideRailGesture();g.begin(p(24),0);assert.equal(g.end(p(24),100),0);
g=createSideRailGesture();g.begin(p(24),0);g.hold(300);assert.equal(g.move(p(70),350),1);assert.equal(g.move(p(116),400),2);assert.equal(g.end(p(116),450),2);
g=createSideRailGesture();g.begin(p(116),0);g.hold(310);assert.equal(g.move(p(70),340),1);assert.equal(g.end(p(24),400),0);
g=createSideRailGesture();g.begin(p(24),0);assert.equal(g.move(p(116),100),2);assert.equal(g.end(p(116),150),2,'scrubbing before the timer must remain usable');
g=createSideRailGesture();g.begin(p(24),0);g.hold(300);assert.equal(g.end({x:200,y:116},350),null);
g=createSideRailGesture();g.begin(p(24),0);g.hold(300);g.cancel();assert.equal(g.end(p(70),400),null);
console.log('Side rail: taps, hold-and-slide both ways, early scrubbing, outside release and cancellation passed.');

// Exercise the component's responder handlers as well as its pure state model.
// In particular, page/window coordinates intentionally disagree with local
// coordinates here, as can happen inside native tab screen roots.
const component=await build({entryPoints:['apps/mobile/src/SideNavigationRail.tsx'],bundle:true,platform:'node',format:'cjs',write:false,external:['react','react-native','@expo/ui/swift-ui','@expo/ui/swift-ui/modifiers']});
const events=[],highlights=[],effects=[],animationValues=[],animationCalls=[];
class Value { constructor(value){this.value=value;animationValues.push(this);} setValue(value){this.value=value;} stopAnimation(){} }
const animated={Value,View:"AnimatedView",spring:(value,config)=>({start(){assert.equal(config.useNativeDriver,true);animationCalls.push(config);value.setValue(config.toValue);}}),parallel:animations=>({start(){animations.forEach(a=>a.start());}})};
const react={createElement:(type,props,...children)=>({type,props:{...props,children}}),useRef:value=>({current:value}),useState:value=>[value,next=>highlights.push(next)],useEffect:callback=>effects.push(callback)};
const native={Animated:animated,Platform:{OS:'ios',Version:26},View:'View',StyleSheet:{absoluteFill:{position:'absolute'},create:value=>value},useWindowDimensions:()=>({width:844,height:390}),PanResponder:{create:handlers=>({panHandlers:handlers})}};
const previous=globalThis.expo;
globalThis.expo={modules:{ExpoUI:{}},getViewConfig:(_module,name)=>['HostView','VStackView','ImageView'].includes(name)};
const module={exports:{}};
const modifiers=new Proxy({},{get:(_target,name)=>(...args)=>({name,args})});
const mockRequire=name=>name==='react'?react:name==='react-native'?native:name.endsWith('/modifiers')?modifiers:{Host:'NativeHost',VStack:'NativeStack',Image:'NativeImage'};
try {new Function('require','module','exports','__DEV__','setTimeout','clearTimeout',component.outputFiles[0].text)(mockRequire,module,module.exports,false,()=>1,()=>{});}finally{globalThis.expo=previous;}
const tree=module.exports.SideNavigationRail({selected:0,onChange:index=>events.push(index),accent:'#7799CC',dark:true,renderIcon:()=>null});
const plane=tree.props.children.at(-1);
assert.equal(plane.props.pointerEvents,'auto');assert.equal(plane.props.collapsable,false);assert.equal(plane.props.style[1].zIndex,2);
const h=plane.props;assert.equal(h.onStartShouldSetPanResponder(),true);
for(const effect of effects)effect();
const down={nativeEvent:{locationX:24,locationY:24,pageX:19000,pageY:8000,touches:[]}};
h.onPanResponderGrant(down,{numberActiveTouches:1});
assert.equal(animationValues[1].value,1.18,"press must lift the glass indicator");
assert.equal(animationValues[2].value,1.04,"press must also expand the whole bar slightly");
h.onPanResponderMove({nativeEvent:{pageX:19000,pageY:8000}},{numberActiveTouches:1,dx:0,dy:92});
assert.equal(highlights.at(-1),2);
assert.equal(animationValues[0].value,92,"indicator must follow the drag continuously");
h.onPanResponderRelease({}, {dx:0,dy:92});assert.deepEqual(events,[2]);
assert.equal(animationValues[1].value,1,"release must restore the resting glass size");
assert.equal(animationValues[2].value,1,"release must also restore the bar size");
h.onPanResponderGrant(down,{numberActiveTouches:1});h.onPanResponderRelease({}, {dx:0,dy:0});assert.deepEqual(events,[2,0]);
h.onPanResponderGrant(down,{numberActiveTouches:1});h.onPanResponderMove({}, {numberActiveTouches:2,dx:0,dy:50});h.onPanResponderRelease({}, {dx:0,dy:50});assert.deepEqual(events,[2,0]);
assert(!JSON.stringify(tree).includes('NativeButton'),'decorative native controls must not compete for touch');
console.log('Side rail touch adapter: topmost local-coordinate input, native decoration, drag, tap and multi-touch cancellation passed.');

assert.equal(sideRailIndicatorOffset(24),0);assert.equal(sideRailIndicatorOffset(47),23);assert.equal(sideRailIndicatorOffset(70),46);assert.equal(sideRailIndicatorOffset(116),92);
assert.equal(sideRailIndicatorOffset(-100),0);assert.equal(sideRailIndicatorOffset(240),92);
g=createSideRailGesture();g.begin(p(110),0);assert.equal(g.move({x:90,y:-30},100),0);assert.equal(g.end({x:90,y:-30},150),0,'diagonal endpoint overshoot should still choose the endpoint');
console.log('Glass feedback: native-driven lift/settle, continuous position and bounded forgiving tracking passed.');

const nodes=value=>!value||typeof value!=='object'?[]:Array.isArray(value)?value.flatMap(nodes):[value,...nodes(value.props?.children)];
for(const accent of ['#7799CC','#FFEE55','#EE5577','#9977CC','#BB9955']) for(const dark of [false,true]) for(const selected of [0,1,2]) {
  const tree=module.exports.SideNavigationRail({selected,onChange(){},accent,dark,renderIcon:()=>null});
  const all=nodes(tree);const hosts=all.filter(n=>n.type==='NativeHost');
  assert.equal(hosts.length,2);
  assert(hosts.every(h=>h.props.ignoreSafeArea==="container"), "screen-edge placement must not independently inset native glass or symbols");
  assert(hosts.every(h=>h.props.style.width===48 && h.props.style.height===140), "native layers share the RN rail frame");
  assert.equal(hosts.filter(h=>h.props.seedColor===accent).length,1,"only the icon host needs the theme tint");
  const iconContainers=all.filter(n=>n.type==='NativeStack'&&n.props.modifiers?.some(m=>m.name==='foregroundColor'));
  assert.equal(iconContainers.length,3);
  for(const [index,n] of iconContainers.entries())assert.equal(n.props.modifiers.find(m=>m.name==='foregroundColor').args[0],index===selected?accent:dark?'#f5f5f7':'#1c1c1e');
  const glass=all.flatMap(n=>n.props?.modifiers??[]).filter(m=>m.name==='glassEffect');
  assert.equal(glass.length,1,'do not restore the moving native-glass layer that turns black on-device');
  const neutral=all.find(n=>n.props?.testID==='side-tab-neutral-selection');
  assert.equal(neutral.props.style.backgroundColor,dark?'rgba(255,255,255,0.14)':'rgba(118,118,128,0.18)');
  const bar=all.find(n=>n.props?.testID==='side-tab-glass-bar');
  assert.equal(bar.props.children.at(-1).props.testID,'side-tab-symbols','native material must never cover the symbols');
  assert.equal(bar.props.children.at(-1).props.style[1].zIndex,1);
  assert.equal(bar.props.children[1].props.testID,'side-tab-glass-selection');
  assert.equal(glass.find(m=>m.args[0].shape==='capsule').args[0].glass.tint,undefined);
}
console.log('Side rail themes: all five accents, light/dark symbols and neutral selection fill, visible themed symbols and one stationary glass backdrop passed.');
