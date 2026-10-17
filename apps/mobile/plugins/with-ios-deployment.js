const {withPodfileProperties,withXcodeProject}=require('@expo/config-plugins');
module.exports=function(config){
 config=withPodfileProperties(config,mod=>{
  mod.modResults['ios.deploymentTarget']='16.4';
  mod.modResults['newArchEnabled']='true';
  return mod;
 });
 return withXcodeProject(config,mod=>{
  for(const item of Object.values(mod.modResults.pbxXCBuildConfigurationSection())) {
   if(item && item.buildSettings && item.buildSettings.IPHONEOS_DEPLOYMENT_TARGET) item.buildSettings.IPHONEOS_DEPLOYMENT_TARGET='16.4';
  }
  return mod;
 });
};
