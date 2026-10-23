const assert = require('node:assert/strict');
const fs = require('node:fs');
const source = fs.readFileSync('apps/mobile/src/app-theme.tsx', 'utf8');
const expected = {sakiko:'#7799CC', arale:'#FFEE55', yuno:'#EE5577', miyako:'#9977CC', uika:'#BB9955'};
const actual = Object.fromEntries([...source.matchAll(/id: "(\w+)", name: "[^"]+", color: "(#[A-F0-9]{6})"/g)].map(m=>[m[1],m[2]]));
assert.deepEqual(actual, expected);
assert.match(source, /useState<ThemeId>\("sakiko"\)/);
assert.match(source, /JSON.stringify\(\{ theme: value \}\)/);
const schemes = JSON.parse(fs.readFileSync('apps/mobile/src/android-material-schemes.json','utf8'));
assert.deepEqual(Object.keys(schemes).sort(), Object.keys(expected).sort());
function luminance(hex) {
  const rgb=hex.slice(1).match(/../g).map(v=>parseInt(v,16)/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4);
  return rgb[0]*.2126+rgb[1]*.7152+rgb[2]*.0722;
}
for(const id of Object.keys(expected)) for(const mode of ['light','dark']) {
  const s=schemes[id][mode];
  for(const [fg,bg] of [['ink','bg'],['ink','panel'],['muted','panel'],['onPrimary','primary'],['onContainer','container']]) {
    const a=luminance(s[fg]),b=luminance(s[bg]);
    assert.ok((Math.max(a,b)+.05)/(Math.min(a,b)+.05)>=4.5, `${id}/${mode}/${fg} contrast`);
  }
}
console.log('Character colors, persisted IDs/default and all 10 MD3 scheme text contrasts passed.');
