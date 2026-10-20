// Record package provenance; does not execute decoders or tests.
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync,mkdirSync,createReadStream} from 'node:fs';
import {resolve,join} from 'node:path';
import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const root=resolve(fileURLToPath(new URL('..',import.meta.url)));
const output=resolve(process.argv[2]??'ios-artifacts');mkdirSync(output,{recursive:true});
const paths=['apps/native-renderer/src/lib.rs','apps/native-renderer/src/auxiliary_balance.rs','packages/core/src/lib.rs','packages/core/src/ac4_pipeline.rs','packages/core/src/av3a_objects.rs','packages/core/mpegh/bridge.c','scripts/prepare-mpegh.mjs','vendor/macindecode-ac4-decode/src/ajoc/reconstruction.rs','crates/sda-native/src/frame_router.rs','crates/sda-native/src/mpegh.rs'];
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const sources=Object.fromEntries(paths.map(path=>[path,sha(readFileSync(join(root,path)))]));
const core=readFileSync(join(root,'packages/core/src/lib.rs'),'utf8');
const ac4=readFileSync(join(root,'vendor/macindecode-ac4-decode/src/ajoc/reconstruction.rs'),'utf8');
const value=(text,name)=>{const match=text.match(new RegExp(name+'\\s*:\\s*usize\\s*=\\s*(\\d+)'));if(!match)throw Error('Missing build metadata constant: '+name);return Number(match[1]);};
const library=resolve(process.env.CARGO_TARGET_DIR??join(root,'crates/sda-native/target'),'aarch64-apple-ios/release/libsda_native.a');
const libraryHash=createHash('sha256');for await(const chunk of createReadStream(library))libraryHash.update(chunk);
const info={sourceCommit:process.env.GITHUB_SHA??execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim(),packagingMode:'device-only-no-tests',target:'aarch64-apple-ios',testsExecuted:false,simulatorBuilt:false,maxDecodedSources:value(core,'MAX_DECODED_SOURCE_CHANNELS'),ac4ReconstructedSources:value(ac4,'MAX_RECONSTRUCTED_OBJECTS'),nativeLibrarySha256:libraryHash.digest('hex'),sources};
writeFileSync(join(output,'build-info.json'),JSON.stringify(info,null,2)+'\n');
console.log('Recorded device package source revision and library provenance.');
