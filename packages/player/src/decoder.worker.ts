import {PerformanceSink} from "./performance-sink";
const perf=new PerformanceSink("decoder");
import {DecoderDiagnostics} from './decoder-diagnostics';
const diagnostic=new DecoderDiagnostics(()=>!!perf.endpoint,value=>perf.diagnostic(value));
import { isStereoMasterFrame } from "./stereo-master.js";
/**
 * Decoder worker script — runs demux + wasm decode off the UI thread.
 * Loaded as a module worker; messages:
 *   in:  { type: "init", wasmUrl }         → init wasm
 *        { type: "open", codec, kind }     → create decoder + demuxer
 *        { type: "push", chunk }           → container bytes (transferable)
 *   out: { type: "track", ... }            → discovered audio track
 *        { type: "frame", frame }          → decoded frame (channels transferred)
 *        { type: "error", message }
 */

import { decoderDiagnosticBuild, configureDecoderDiagnostics, drainDecoderCheckpoints, initCore, SdaDecoder, type CodecName, type DecodedFrameData, type ObjectEvent } from "@sda/core";
import { createDemuxer, sniffContainer, type BwfMetadata, type BinauralRenderMetadata, type ContainerKind, type Demuxer, type DemuxedAudioPacket } from "@sda/demux";
import { canCoalesceObjectEvent } from "./control.js";
import { LoudnessMeter } from "./bs1770.js";
import { FrameBatcher } from "./frame-batcher.js";
import { AlacResampler } from "./alac-resampler.js";
import {initMpegh, isMhas, MpeghDecoder} from "../../core/src/mpegh.js";

/** Minimal worker global typing (avoids DOM/WebWorker lib conflicts). */
declare const self: {
  postMessage(message: unknown, transfer?: Transferable[]): void;
  onmessage: ((e: MessageEvent) => void) | null;
};

import {initIamf,isIamf,IamfDecoder} from "../../core/src/iamf.js";

let decoder: SdaDecoder | IamfDecoder | MpeghDecoder | null = null;
let demuxer: Demuxer | null = null;
let sniffPrefix = new Uint8Array(0);
let bwfMetadata: BwfMetadata | undefined;
let decoderConfigurationError: string | null = null;
let loudnessMeter: LoudnessMeter | null = null;
let loudnessPostCounter = 0;
const lastObjectTargets = new Map<number, ObjectEvent>();
let frameBatcher = new FrameBatcher(postFrame);
let outputSampleRate: number | undefined;
let resampler: AlacResampler | null = null;
let decodedFrames: DecodedFrameData[] = [];
let epoch=0;
let perfDecodeStartMs=0;
function post(message:Record<string,unknown>, transfer:Transferable[] = []):void { if(message.type==='error'){const trace=drainDecoderCheckpoints();for(const item of trace.events)diagnostic.record('checkpoint',item);diagnostic.record('failure',{epoch,checkpointDropped:trace.dropped,...decoderDiagnosticBuild});}self.postMessage({...message,epoch},transfer); }

function compactObjectEvents(frame: DecodedFrameData): void {
  const objectIds = new Set<number>();
  for (const declaration of frame.objectChannels) objectIds.add(declaration.id);
  if (!frame.labels.some((label) => label.startsWith("Obj_"))) lastObjectTargets.clear();
  else if (objectIds.size > 0) {
    for (const id of lastObjectTargets.keys()) {
      if (!objectIds.has(id)) lastObjectTargets.delete(id);
    }
  }
  frame.events = frame.events.filter((event) => {
    if (canCoalesceObjectEvent(lastObjectTargets.get(event.id), event)) return false;
    lastObjectTargets.set(event.id, event);
    return true;
  });
}

function measureMpeghReference(channels:Float32Array[],sampleRate:number):void {
  loudnessMeter ??= new LoudnessMeter(sampleRate,2);
  const at=perf.endpoint?performance.now():null;
  loudnessMeter.push(channels);
  if(at!==null)perf.record("decode.loudness", "mpegh", performance.now()-at,channels[0]?.length??0);
}

function postFrame(frame: DecodedFrameData): void {
  if(frame.codec === "mpegh" && loudnessMeter && ++loudnessPostCounter % 8 === 0) frame.loudness=loudnessMeter.integrated();
  // BS.1770-4 measurement for content without codec loudness metadata (e.g.
  // ALAC/stereo). Attached on a subset of frames to bound message overhead.
  // ADM tracks are unrendered sources, not BS.1770 speaker channels. Applying
  // channel-layout weights to 118 objects is both incorrect and very costly.
  if (frame.codec !== "mpegh" && isStereoMasterFrame(frame) && frame.channels[0]?.length) {
    loudnessMeter ??= new LoudnessMeter(frame.sampleRate, frame.channels.length);
    const at=perf.endpoint?performance.now():null;
    loudnessMeter.push(frame.channels);
    if(at!==null)perf.record("decode.loudness",frame.codec,performance.now()-at,frame.samplesPerChannel);
    if (++loudnessPostCounter % 8 === 0) frame.loudness = loudnessMeter.integrated();
  }
  perf.record("decode.output_audio",frame.codec,0,frame.samplesPerChannel/frame.sampleRate);
  if(perf.endpoint)perf.trace({type:"decodeTrace",sample:frame.samplePos,startedMs:perfDecodeStartMs,epoch,ids:frame.labels.map((label,i)=>label.startsWith("Obj_")?`obj:${label.slice(4)}`:`bed:${i}`)});
  post(
    { type: "frame", frame },
    frame.channels.map((c) => c.buffer),
  );
}

function drainFrames(): void {
  if (!decoder) return;
  while (true) {
    const perfStart=perf.endpoint?performance.now():null;
    const frame = decoder.nextFrame();
    if(perfStart!==null)perf.record("decode.next_frame",frame?.codec??"drain",performance.now()-perfStart,frame?frame.samplesPerChannel/frame.sampleRate:0);
    if (!frame) break;
    diagnostic.record('frame',{codec:frame.codec,sample:frame.samplePos,samples:frame.samplesPerChannel,rate:frame.sampleRate,channels:frame.labels.length,objects:frame.objectChannels.length,epoch});
    acceptDecodedFrame(frame);
  }
  for (const message of decoder.drainErrors()) {
    post({ type: "error", message });
  }
}

function decodePacket(packet: DemuxedAudioPacket): void {
  diagnostic.record('packet',{bytes:packet.frames.reduce((n,frame)=>n+frame.byteLength,0),units:packet.frames.length,epoch});
  if (!decoder) {
    if (!decoderConfigurationError) post({type:'error',message:'audio packets arrived before the decoder was configured'});
    return;
  }
  const packetStarted=perf.endpoint?performance.now():null;
  try { for (const au of packet.frames) decoder.push(au); }
  finally { if(packetStarted!==null)perf.record("decode.packet_push", "codec", performance.now()-packetStarted, packet.frames.length); }
  drainFrames();
}

async function processDecodedFrames(): Promise<void> {
  for (const frame of decodedFrames.splice(0)) {
    const resampleStarted=perf.endpoint?performance.now():null;
    const output = outputSampleRate && frame.sampleRate !== outputSampleRate
      ? await (resampler ??= new AlacResampler(outputSampleRate)).push(frame)
      : frame;
    if(resampleStarted!==null)perf.record("decode.resample",frame.codec,performance.now()-resampleStarted,frame.samplesPerChannel);
    if (output) {
      compactObjectEvents(output);
      frameBatcher.push(output);
    }
  }
}

function acceptDecodedFrame(frame: DecodedFrameData): void {
  if (outputSampleRate && frame.sampleRate !== outputSampleRate) {
    decodedFrames.push(frame);
    return;
  }
  // Do not hold native-rate PCM until the entire compressed input chunk has
  // decoded: the audio thread must receive frames while decoding continues.
  compactObjectEvents(frame);
  frameBatcher.push(frame);
}

// WASM resampler initialization is async: keep open/push/flush in port order.
let messages = Promise.resolve();
self.onmessage = (e: MessageEvent) => {
  const received=performance.now();
  messages = messages.then(() => handleMessage(e,received));
};

async function handleMessage(e: MessageEvent,received:number): Promise<void> {
  const msg = e.data;
  if(msg.type==="performance"){perf.endpoint=typeof msg.endpoint==="string"?msg.endpoint:null;configureDecoderDiagnostics(!!perf.endpoint);return;}
  const perfStarted=perf.endpoint?performance.now():null;
  if(perfStarted!==null){perfDecodeStartMs=performance.timeOrigin+perfStarted;perf.record("decode.queue_wait",msg.type,perfStarted-received);}
  try {
    if (msg.type === "open") epoch=msg.epoch ?? 0;
    else if (msg.type !== "init" && (msg.epoch ?? 0) !== epoch) return;
    switch (msg.type) {
    case "init": {
      await initCore();
      post({ type: "ready" });
      break;
    }
    case "open": {
      drainDecoderCheckpoints();diagnostic.reset();diagnostic.record('open',{codec:msg.codec,epoch});
      decoder?.free();
      resampler?.destroy();
      resampler = null;
      decodedFrames = [];
      outputSampleRate = msg.outputSampleRate;
      bwfMetadata = msg.bwfMetadata;
      frameBatcher = new FrameBatcher(postFrame);
      loudnessMeter = null;
      loudnessPostCounter = 0;
      // Keep the existing immediate decoder for raw and legacy MP4 streams.
      // ALAC replaces it in onTrack before MP4Box starts delivering packets,
      // because only the discovered track carries its required codec cookie.
      decoder = new SdaDecoder(msg.codec as Exclude<CodecName, "alac">);
      decoderConfigurationError = null;
      sniffPrefix = new Uint8Array(0);
      demuxer = null; // created on first push, after sniffing
      lastObjectTargets.clear();
      break;
    }
    case "flush": {
      diagnostic.record('flush',{epoch});
      if (sniffPrefix.length) throw new Error("Truncated media header");
      demuxer?.flush();
      decoder?.flush();
      drainFrames();
      await processDecodedFrames();
      const tail = resampler?.finish();
      if (tail) {
        compactObjectEvents(tail);
        frameBatcher.push(tail);
      }
      frameBatcher.flush();
      if (loudnessMeter) post({ type: "loudness-complete", loudness: loudnessMeter.integrated() });
      post({ type: "flushed" });
      break;
    }
    case "push": {
      let chunk = new Uint8Array(msg.chunk as ArrayBuffer);
      if (!demuxer) {
        if (sniffPrefix.length) {
          const joined = new Uint8Array(sniffPrefix.length + chunk.length);
          joined.set(sniffPrefix); joined.set(chunk, sniffPrefix.length); chunk = joined;
          sniffPrefix = new Uint8Array(0);
        }
        if (chunk.length < 16) {
          sniffPrefix = chunk;
          post({type: "push-ack", sequence: msg.sequence});
          break;
        }
        const kind: ContainerKind = msg.kind ?? (bwfMetadata ? "bwf" : sniffContainer(chunk));
        if(kind === "mp4" || isMhas(chunk)) await initMpegh();
        if(kind === "raw" && isMhas(chunk)) {decoder?.free();decoder=new MpeghDecoder(false,undefined,measureMpeghReference,true);}
        if(kind === "raw" && isIamf(chunk)) {await initIamf();decoder?.free();decoder=new IamfDecoder();}
        demuxer = createDemuxer(kind, {
          onTrack: (t) => {
            if(t.codec === "mha1" || t.codec === "mhm1") {
              decoder?.free();decoder=new MpeghDecoder(t.codec === "mha1",t.decoderConfig,measureMpeghReference,true);
              decoderConfigurationError=null;
            }
            if (["dts", "dtsc", "dtsh", "dtsl", "dtse"].includes(t.codec)) {
              decoder?.free();
              decoder = new SdaDecoder("dts");
              decoderConfigurationError = null;
            }
            if (t.codec === "ac-4") {
              decoder?.free();
              decoder = new SdaDecoder("ac4");
              decoderConfigurationError = null;
            }
            if (t.codec === "adm" || t.codec === "pcm") {
              decoder?.free();
              decoder = null;
              decoderConfigurationError = null;
            }
            if (t.codec === "alac") {
              try {
                if (!t.decoderConfig) throw new Error("MP4 ALAC track is missing its decoder configuration");
                decoder?.free();
                decoder = SdaDecoder.withConfig("alac", t.decoderConfig);
                decoderConfigurationError = null;
                loudnessMeter = null;
                loudnessPostCounter = 0;
              } catch (error) {
                decoder?.free();
                decoder = null;
                decoderConfigurationError = error instanceof Error ? error.message : String(error);
                post({ type: "error", message: decoderConfigurationError });
              }
            }
            post(
              { type: "track", track: t },
              t.coverArt ? [t.coverArt.bytes.buffer] : [],
            );
          },
          onPacket: decodePacket,
          onError: (m) => post({ type: "error", message: m }),
          onPcmFrame: acceptDecodedFrame,
          onBinauralMetadata: (metadata: BinauralRenderMetadata) => post({ type: "binaural-metadata", metadata }),
        }, bwfMetadata);
      }
      demuxer.push(chunk, Number.isSafeInteger(msg.fileStart) ? msg.fileStart : undefined);
      drainFrames();
      await processDecodedFrames();
      frameBatcher.flush();
      post({ type: "push-ack", sequence: msg.sequence });
      break;
    }
    }
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    post({ type: "error", message });
    if (msg.type === "push") post({ type: "push-ack", sequence: msg.sequence, error: message });
  } finally { if(perfStarted!==null)perf.record("decode.message_including_wait",msg.type,performance.now()-perfStarted); }
}

export {};
