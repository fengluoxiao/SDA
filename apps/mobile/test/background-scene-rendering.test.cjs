const fs=require('node:fs');const path=require('node:path');const assert=require('node:assert/strict');const {test}=require('node:test');const ts=require('typescript');
const file=path.resolve(__dirname,'../src/MobileObjectScene.tsx');const source=ts.createSourceFile(file,fs.readFileSync(file,'utf8'),ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
const component=source.statements.find(n=>ts.isFunctionDeclaration(n)&&n.name?.text==='MobileObjectScene');assert.ok(component);
const prefix=[];for(const statement of component.body.statements){if(statement.getText(source).startsWith('const view ='))break;prefix.push(statement.getText(source));}
let loop;function visit(n){if(ts.isJsxAttribute(n)&&n.name.getText(source)==='frameloop')loop=n.initializer.expression.getText(source);ts.forEachChild(n,visit);}visit(component);assert.ok(loop);
function harness(os){
 let current='active',value;let subscribed=false,cleanup,removes=0;const listeners=new Set();
 const AppState={get currentState(){return current},addEventListener(name,fn){assert.equal(name,'change');listeners.add(fn);return{remove(){listeners.delete(fn);removes++;}}}};
 const React={useState(initial){if(value===undefined)value=initial;return[value,next=>{value=next}]},useEffect(fn){if(!subscribed){subscribed=true;cleanup=fn();}}};
 const module={exports:{}};const js=ts.transpileModule(`module.exports=(active)=>{${prefix.join('\n')} return {active:renderActive,loop:${loop}}}`,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
 new Function('React','AppState','Platform','module',js)(React,AppState,{OS:os},module);
 return{render:module.exports,change(state){current=state;for(const fn of listeners)fn(state)},cleanup(){cleanup?.()},listeners,removes:()=>removes};
}
test('Android and iOS stop background and hidden-page rendering, then resume the same scene',()=>{
 for(const os of ['android','ios']){
  const h=harness(os);assert.deepEqual(h.render(true),{active:true,loop:'always'});
  assert.deepEqual(h.render(false),{active:false,loop:'never'});
  for(const state of ['background','inactive']){h.change(state);assert.deepEqual(h.render(true),{active:false,loop:'never'});}
  h.change('active');assert.deepEqual(h.render(true),{active:true,loop:'always'});assert.equal(h.listeners.size,1);
  h.cleanup();assert.equal(h.listeners.size,0);assert.equal(h.removes(),1);
 }
});
