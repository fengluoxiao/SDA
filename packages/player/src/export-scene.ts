import {LAYOUTS} from "../../renderer/src/layouts";
import {sphericalToAdm} from "../../renderer/src/coords";
import {SDA_UPMIX_LEVEL_TRIM_DB} from "./alac-stereo-upmix";

const bed = [
  ["FL","FrontLeft",0,[.8,0]], ["FR","FrontRight",1,[0,.8]],
  ["FC","Center",2,[.25,.25]], ["LFE","LFE",3,[.125,.125]],
  ["BL","RearLeft",6,[.1,-.1]], ["BR","RearRight",7,[-.1,.1]],
  ["SL","SurroundLeft",4,[.17,-.17]], ["SR","SurroundRight",5,[-.17,.17]],
  ["TFL","TopFrontLeft",8,[.07,-.07]], ["TFR","TopFrontRight",9,[-.07,.07]],
  ["TBL","TopRearLeft",10,[.05,-.05]], ["TBR","TopRearRight",11,[-.05,.05]],
] as const;
export interface ExportRenderStats {
  frames:number;sourceSampleRate:number;sourceLufs:number|null;programmeGainDb:number;
  programmeMakeupDb:number;volumeBalanceEnabled:boolean;spatialBalanceEnabled:boolean;
  peakHeadroomGainDb:number;truePeakDbtp:number|null;rendererRevision?:string;
}
export function exportRenderMetadata(snapshot:Record<string,unknown>|null|undefined,stats:ExportRenderStats,assets:Record<string,unknown>|null=null) {
  const layout=LAYOUTS["7.1.4"];
  const channels=bed.map(([label,name,sourceIndex,coefficients],index)=>{
    const position=layout.find(speaker=>speaker.name===name)!;
    return {id:`sda:bed:${label}`,kind:"bed",pcmChannel:index+1,label,name,
      sourceId:`bed:${sourceIndex}`,sourceChannel:sourceIndex+1,isLfe:position.isLfe===true,
      position:{azimuth:position.azimuth,elevation:position.elevation,distance:position.distance},
      cartesian:sphericalToAdm(position),gainDb:0,
      sourceMapping:{kind:name==="LFE"?"filtered-stereo-matrix":"stereo-matrix",inputs:["source:L","source:R"],coefficients:[...coefficients],
        ...(name==="LFE"?{lowpassHz:120}:{}),fixedTrimDb:SDA_UPMIX_LEVEL_TRIM_DB,precision:"f32"},
      trackUid:`ATU_${(index+1).toString(16).toUpperCase().padStart(8,"0")}`,
      channelFormatId:`AC_0001${(0x1001+index).toString(16).toUpperCase()}`,
      trackFormatId:`AT_0001${(0x1001+index).toString(16).toUpperCase()}_01`,
      streamFormatId:`AS_0001${(0x1001+index).toString(16).toUpperCase()}`,
    };
  });
  return {schema:"sda.render-metadata",version:1,profile:"SDAR Bed Channel",
    interchange:{schema:"sda.interchange",version:1,rendererContract:{id:"sda.authored-scene",version:1},requiredFeatures:["bed-scene-v1","file-processing-v1","embedded-render-assets-v1"],processingOrder:"source-processing-then-file-render",bakedProcessing:"baked",runtimeParameters:"playbackSnapshot",resources:"assets",unsupportedRequiredFeature:"error"},
    coordinateSystem:{spherical:"azimuth-positive-left; elevation-positive-up; distance-reference-ring",cartesian:"ADM: X-right, Y-front, Z-up"},
    scene:{kind:"bed",layout:"7.1.4",channels,objects:[],automation:[],durationSamples:stats.frames,sampleRate:48000},
    playbackSnapshot:snapshot??{available:false,reason:"export invoked without a playback snapshot"},
    output:{sampleRate:48000,channels:12,bitsPerSample:24,pcmFormat:"signed-little-endian",channelMask:0x2d63f,sourceSampleRate:stats.sourceSampleRate},
    baked:{upmix:{applied:true,algorithm:"native-mobile-stereo-matrix-v1",levelTrimDb:SDA_UPMIX_LEVEL_TRIM_DB},
      volumeBalance:{applied:true,enabled:stats.volumeBalanceEnabled,policy:"source-rate-full-track-reference-v1",targetLufs:-18,sourceLufs:stats.sourceLufs,programmeGainDb:stats.programmeGainDb,makeupDb:stats.programmeMakeupDb},
      spatialBalance:{applied:true,enabled:stats.spatialBalanceEnabled,algorithm:"directional-spectral-redistribution-v1",clarityShelf:{frequencyHz:4000,gainDb:stats.spatialBalanceEnabled?0.6:0}},
      peakProtection:{policy:"whole-file-linked-headroom-v1",ceilingDbtp:-1,headroomGainDb:stats.peakHeadroomGainDb,truePeakDbtp:stats.truePeakDbtp},dither:"TPDF PCM24"},
    playbackPolicy:{authority:"file-authored",applyBakedProcessingAgain:false,overrideUserEnhancementPresets:true,unsupportedProcessing:"error",mobileDefaultLayout:"7.1.4",listenerSystemVolume:"user-controlled",listenerHeadPose:"user-controlled"},
    assets,
    runtimeRecipe:{source:"playbackSnapshot",operations:["HRTF","headphone EQ","room simulation","monitor hardware","head tracking","speaker routing"],automationRecorded:false},
    rendererRevision:stats.rendererRevision??(snapshot?.native as {rendererRevision?:string}|undefined)?.rendererRevision??null,
  };
}
const xml=(value:unknown)=>String(value).replace(/[<>&"']/g,char=>({"<":"&lt;",">":"&gt;","&":"&amp;",'"':"&quot;","'":"&apos;"}[char]!));
export function admBedMetadata(render:ReturnType<typeof exportRenderMetadata>,title="SDA Bed Master") {
  const channels=render.scene.channels,pack="AP_00011001";
  const seconds=Math.floor(render.scene.durationSamples/48000),fraction=((render.scene.durationSamples%48000)/48000).toFixed(9).slice(1);
  const duration=`${String(Math.floor(seconds/3600)).padStart(2,"0")}:${String(Math.floor(seconds/60)%60).padStart(2,"0")}:${String(seconds%60).padStart(2,"0")}${fraction}`;
  const uidRefs=channels.map(channel=>`<audioTrackUIDRef>${channel.trackUid}</audioTrackUIDRef>`).join("");
  const elements=channels.map(channel=>{
    const blockId=channel.channelFormatId.replace("AC_","AB_")+"_00000001";
    const speakerLabel=channel.isLfe?"LFE1":`${channel.position.elevation?"U":"M"}${channel.position.azimuth<0?"-":"+"}${String(Math.abs(channel.position.azimuth)).padStart(3,"0")}`;
    return `<audioChannelFormat audioChannelFormatID="${channel.channelFormatId}" audioChannelFormatName="${xml(channel.name)}" typeLabel="0001" typeDefinition="DirectSpeakers"><audioBlockFormat audioBlockFormatID="${blockId}" rtime="00:00:00.000000000" duration="${duration}"><speakerLabel>${speakerLabel}</speakerLabel><position coordinate="azimuth">${channel.position.azimuth}</position><position coordinate="elevation">${channel.position.elevation}</position><position coordinate="distance">${channel.position.distance}</position></audioBlockFormat></audioChannelFormat>
<audioStreamFormat audioStreamFormatID="${channel.streamFormatId}" audioStreamFormatName="${channel.label}" formatLabel="0001" formatDefinition="PCM"><audioChannelFormatIDRef>${channel.channelFormatId}</audioChannelFormatIDRef><audioPackFormatIDRef>${pack}</audioPackFormatIDRef><audioTrackFormatIDRef>${channel.trackFormatId}</audioTrackFormatIDRef></audioStreamFormat>
<audioTrackFormat audioTrackFormatID="${channel.trackFormatId}" audioTrackFormatName="${channel.label}" formatLabel="0001" formatDefinition="PCM"><audioStreamFormatIDRef>${channel.streamFormatId}</audioStreamFormatIDRef></audioTrackFormat>
<audioTrackUID UID="${channel.trackUid}" sampleRate="48000" bitDepth="24"><audioTrackFormatIDRef>${channel.trackFormatId}</audioTrackFormatIDRef><audioPackFormatIDRef>${pack}</audioPackFormatIDRef></audioTrackUID>`;
  }).join("\n");
  const axml=Buffer.from(`<?xml version="1.0" encoding="UTF-8"?><ebuCoreMain xmlns="urn:ebu:metadata-schema:ebuCore_2017"><coreMetadata><format><audioFormatExtended version="ITU-R_BS.2076-2"><audioProgramme audioProgrammeID="APR_1001" audioProgrammeName="${xml(title)}" start="00:00:00.000000000" end="${duration}"><audioContentIDRef>ACO_1001</audioContentIDRef></audioProgramme><audioContent audioContentID="ACO_1001" audioContentName="SDA Bed"><audioObjectIDRef>AO_1001</audioObjectIDRef></audioContent><audioObject audioObjectID="AO_1001" audioObjectName="SDA 7.1.4 Bed" start="00:00:00.000000000" duration="${duration}"><audioPackFormatIDRef>${pack}</audioPackFormatIDRef>${uidRefs}</audioObject><audioPackFormat audioPackFormatID="${pack}" audioPackFormatName="SDA 7.1.4" typeLabel="0001" typeDefinition="DirectSpeakers">${channels.map(channel=>`<audioChannelFormatIDRef>${channel.channelFormatId}</audioChannelFormatIDRef>`).join("")}</audioPackFormat>${elements}</audioFormatExtended></format></coreMetadata></ebuCoreMain>`,"utf8");
  const chna=Buffer.alloc(4+40*channels.length);chna.writeUInt16LE(channels.length,0);chna.writeUInt16LE(channels.length,2);
  channels.forEach((channel,index)=>{const offset=4+40*index;chna.writeUInt16LE(index+1,offset);chna.write(channel.trackUid,offset+2,12,"ascii");chna.write(channel.trackFormatId,offset+14,14,"ascii");chna.write(pack,offset+28,11,"ascii");});
  return {axml,chna};
}
