const assert=require('node:assert/strict'),{test}=require('node:test'),fs=require('node:fs'),ts=require('typescript');
const file=ts.createSourceFile('IOSPlayer.tsx',fs.readFileSync('apps/mobile/src/IOSPlayer.tsx','utf8'),ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
const elements=[];function visit(node){if(ts.isJsxElement(node))elements.push(node);ts.forEachChild(node,visit);}visit(file);
test('the cover is a stationary sibling, never a child of the landscape lyric animation',()=>{
 const pane=elements.find(node=>node.openingElement.tagName.getText(file)==='LandscapeLyricsPager');assert(pane);
 const row=pane.parent;assert(ts.isJsxElement(row));
 const cover=row.children.find(node=>ts.isJsxElement(node)&&node.openingElement.tagName.getText(file)==='Pressable'&&node.openingElement.getText(file).includes('onPress={p.chooseFile}'));
 assert(cover,'the existing album artwork/import control stays outside the lyric pager');assert(row.children.indexOf(cover)<row.children.indexOf(pane));
 assert.match(cover.getText(file),/tablet-complete-artwork/);assert.doesNotMatch(pane.getText(file),/tablet-complete-artwork|LyricsBackdrop|ArtworkEdgeBlend/);
 assert.match(pane.openingElement.getText(file),/enabled=\{landscapePlayer\}/);
 const outer=elements.find(node=>node.openingElement.tagName.getText(file)==='PortraitLyricsPager');assert.match(outer.openingElement.getText(file),/enabled=\{!landscapePlayer\}/,'the full-width portrait pager must be disabled in landscape');
});
test('landscape never installs the full-page blurred lyrics background over the sharp cover',()=>{
 const text=file.getFullText();assert.match(text,/pageBackgrounds=\{\[!landscapePlayer && <LyricsBackdrop/);
});
test('the phone artwork stays mounted across tablet-style and landscape presentations',()=>{
 const plane=elements.find(node=>node.openingElement.getText(file).includes('testID="persistent-artwork-plane"'));assert(plane);
 assert.match(plane.openingElement.getText(file),/opacity: tabletPlayer \? 0 : 1/);
 assert(plane.getText(file).includes('<Animated.Image'),'the preloaded sharp artwork is retained in its stable native plane');
});
