const assert = require('node:assert/strict');
const {sharedSystemOutput} = require('./system-audio-output.cjs');
const current={actualId:'realtek',actualName:'Realtek',mode:'exclusive'};
assert.deepEqual(sharedSystemOutput(current),{deviceId:'realtek',exclusive:false,remoteCompatible:false});
// Following Windows' virtual default would create feedback: pin the real output.
assert.deepEqual(sharedSystemOutput(current,{deviceId:null,remoteCompatible:true,exclusive:true}),sharedSystemOutput(current));
assert.equal(sharedSystemOutput(current,{deviceId:'headphones',exclusive:true},[{id:'headphones',name:'Headphones'}]).deviceId,'headphones');
assert.throws(()=>sharedSystemOutput(current,{deviceId:'virtual'},[{id:'virtual',name:'SDA Spatial Bitstream Input'}]));
assert.throws(()=>sharedSystemOutput({actualId:'asio:test',actualName:'ASIO test'}));
assert.throws(()=>sharedSystemOutput(current,{deviceId:'missing'},[]));
console.log('PASS system audio output: auto shared, physical endpoint pinned, feedback rejected');
