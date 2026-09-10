const {parentPort,workerData:d}=require("node:worker_threads");
Promise.resolve().then(()=>{
 if(d.kind==='generate')return require('./generated-hrtf.cjs').generate(d.parameters,d.assessment,d.store);
 if(d.kind==='archive'){const library=require('./personal-hrtf-library.cjs');if(d.action==='export')return library.exportArchive(d.store,d.id,d.directory);if(d.action==='copy')return library.copy(d.store,d.id,d.name);throw new Error('未知档案操作');}
 if(typeof d.sourcePath==='string'&&/\.phrtf$/i.test(d.sourcePath))return require('./personal-hrtf-library.cjs').importArchive(d.store,d.sourcePath);
 return require('./personal-hrtf.cjs').importSofa(d.sourcePath,d.store);
}).then(value=>parentPort.postMessage({value}),error=>parentPort.postMessage({error:String(error)}));
