const { cp, rm, access, readFile, readdir } = require('node:fs/promises');
const { join } = require('node:path');

// electron-builder's asarUnpack matcher requires every input to be under appDir.
// Keep the runtime path app.asar/web, but stage the sibling Vite build first.
async function stageWeb() {
  // Fail packaging rather than silently shipping an old/missing asset family.
  const { activeExperiment, verifySharedExperiment } = await import('../../scripts/stage-shared-render-experiment.mjs');
  if (activeExperiment()) {
    const executable=join(__dirname,'native-renderer',process.platform==='win32'?'SdaNativeRenderer.exe':'SdaNativeRenderer');
    await access(executable);
    if(activeExperiment().measured && !(await readFile(executable)).includes(Buffer.from('Shared HRTFHQ depth profile v1')))
      throw new Error('Native renderer is stale; rebuild it before packaging the shared HRTFHQ resources');
    verifySharedExperiment(join(__dirname,'native-renderer','hrtf-assets'));
  }
  const source = join(__dirname, '..', 'web', 'dist');
  const destination = join(__dirname, 'web');
  await access(join(source, 'index.html'));
  if(activeExperiment()?.measured) {
    const files=await readdir(join(source,'assets'));
    const scripts=await Promise.all(files.filter(name=>name.endsWith('.js')).map(name=>readFile(join(source,'assets',name),'utf8')));
    if(!scripts.some(text=>text.includes('sda-shared-hrtf-profile')))
      throw new Error('Desktop web bundle is stale; rebuild the web app for the shared HRTFHQ defaults');
  }
  await rm(destination, { recursive: true, force: true });
  await cp(source, destination, { recursive: true });
  await cp(join(__dirname,'../../packages/core/pkg-node'),join(__dirname,'performance-core'),{recursive:true});
}

// Multi-architecture packaging can invoke beforePack concurrently.
let staging;
module.exports = () => (staging ??= stageWeb());
