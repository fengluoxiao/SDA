const assert=require('node:assert/strict'),fs=require('node:fs'),{transformSync}=require('esbuild');
const element=(type,props,...children)=>({type,props:{...props,children:children.length===1?children[0]:children}});
let cursor=0,selected='yuno',fallback=0;const state=[],selections=[];
const react={createElement:element,createContext:value=>({value}),useState:initial=>{const i=cursor++;if(!(i in state))state[i]=initial;return[state[i],v=>{state[i]=typeof v==='function'?v(state[i]):v}];}};
const catalog={exports:{}};
new Function('require','module','exports',transformSync(fs.readFileSync('apps/mobile/src/app-theme.tsx','utf8'),{loader:'tsx',format:'cjs'}).code)(name=>{const modules={react,'expo-file-system/legacy':{documentDirectory:null}};assert(name in modules,name);return modules[name]},catalog,catalog.exports);
const themes=catalog.exports.CHARACTER_THEMES;
const theme={CHARACTER_THEMES:themes,validTheme:catalog.exports.validTheme,useAppTheme:()=>({id:selected,accent:themes.find(x=>x.id===selected).color,setTheme:id=>{selected=id;selections.push(id)}})};
const palette={ink:'#111',primary:'#456',muted:'#777'};
const dependencies={react,expo:{requireNativeView:()=> 'FrameworkMenuAnchor'},'react-native':{Platform:{OS:'android'},Pressable:'Pressable',Text:'Text',View:'View',StyleSheet:{create:x=>x}},'./app-theme':theme,'./android-material-theme':{androidMaterialTheme:()=>palette},'./AndroidSymbol':{AndroidSymbol:'AndroidSymbol'}};
const code=transformSync(fs.readFileSync('apps/mobile/src/AndroidThemeMenu.tsx','utf8'),{loader:'tsx',format:'cjs'}).code;
function load(native){const m={exports:{}};new Function('require','module','exports','globalThis',code)(name=>{assert(name in dependencies,name);return dependencies[name]},m,m.exports,{expo:{modules:native?{SdaAndroidChrome:{}}:{}}});return m.exports.AndroidThemeMenu;}
function nodes(node,type){if(!node||typeof node!=='object')return[];if(Array.isArray(node))return node.flatMap(x=>nodes(x,type));return[...(node.type===type?[node]:[]),...nodes(node.props?.children,type)]}
const Component=load(true);const render=(dark=false)=>{cursor=0;return Component({dark,onFallbackOpen:()=>fallback++})};
let tree=render();let anchor=nodes(tree,'FrameworkMenuAnchor')[0];assert.equal(anchor.props.presentation,'theme-anchor');assert.equal(anchor.props.themeMenuRequest,0);assert.equal(anchor.props.themeMenuSelected,'yuno');assert.equal(anchor.props.accentColorValue,palette.primary,'pass current application control tint to the framework bridge');
assert.deepEqual(JSON.parse(anchor.props.themeMenuItems),themes.map(t=>({id:t.id,label:t.name,color:t.color})));assert.equal(nodes(tree,'Pressable').length,1);
nodes(tree,'Pressable')[0].props.onPress();tree=render(true);anchor=nodes(tree,'FrameworkMenuAnchor')[0];assert.equal(anchor.props.themeMenuRequest,1);assert.equal(anchor.props.dark,true);assert.equal(nodes(tree,'Pressable')[0].props.accessibilityState.expanded,true);
nodes(tree,'Pressable')[0].props.onPress();tree=render();assert.equal(nodes(tree,'FrameworkMenuAnchor')[0].props.themeMenuRequest,2,'a click must reach the framework even if JS dismissal state lags');
anchor.props.onThemeSelect({nativeEvent:{id:themes[0].id}});tree=render();assert.equal(selected,themes[0].id);assert.deepEqual(selections,[themes[0].id]);assert.equal(nodes(tree,'Pressable')[0].props.accessibilityState.expanded,false);
assert.ok(JSON.stringify(nodes(tree,'Pressable')[0]).includes(themes[0].name));assert.equal(nodes(tree,'FrameworkMenuAnchor')[0].props.themeMenuRequest,2,'a theme rerender must not reopen the popup');
nodes(tree,'Pressable')[0].props.onPress();tree=render();anchor=nodes(tree,'FrameworkMenuAnchor')[0];assert.equal(anchor.props.themeMenuRequest,3);anchor.props.onThemeDismiss();tree=render();assert.equal(nodes(tree,'Pressable')[0].props.accessibilityState.expanded,false);
for(const id of ['unknown','',null,3])anchor.props.onThemeSelect({nativeEvent:{id}});assert.deepEqual(selections,[themes[0].id]);
cursor=0;const fallbackTree=load(false)({dark:false,onFallbackOpen:()=>fallback++});assert.equal(nodes(fallbackTree,'FrameworkMenuAnchor').length,0);nodes(fallbackTree,'Pressable')[0].props.onPress();assert.equal(fallback,1);
const nativeSource=fs.readFileSync('apps/mobile/modules/sda-core/android/src/main/java/app/sda/mobile/sda/SdaAndroidChromeModule.kt','utf8');assert.match(nativeSource,/import android\.widget\.PopupMenu/);assert.match(nativeSource,/PopupMenu\(popupContext, this, Gravity\.END\)/);
console.log('Android framework theme menu: one-row trigger, anchored request, current item, native selection/dismissal, invalid-event rejection and no-module fallback passed.');
