"use strict";
const defaults=Object.freeze({rear:1,height:1,bassDb:0,trebleDb:0});
function validate(value){
 if(!value || typeof value!=="object" || Array.isArray(value) || Object.keys(value).length!==4 || Object.keys(value).some(k=>!Object.hasOwn(defaults,k)))throw Error("试听参数无效");
 for(const k of Object.keys(defaults)){
  const n=value[k],direction=k==="rear"||k==="height";
  if(typeof n!=="number"||!Number.isFinite(n)||(direction?n<.5||n>1.5:n< -3||n>3))throw Error("试听参数超出范围");
 }
 return {rear:value.rear,height:value.height,bassDb:value.bassDb,trebleDb:value.trebleDb};
}
function read(value){try{return validate(value);}catch{return {...defaults};}}
module.exports={defaults,validate,read};
