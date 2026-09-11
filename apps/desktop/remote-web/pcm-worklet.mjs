import { StereoPcmBuffer } from "./pcm-buffer.mjs";
class SdaRemotePcm extends AudioWorkletProcessor {
  constructor() {
    super(); this.fifo = new StereoPcmBuffer(); this.ticks = 0; this.failed = false;
    this.port.onmessage = ({data}) => {
      try {
        if (data.type === "pcm") this.fifo.push(new Float32Array(data.samples));
        else if (data.type === "reset") this.fifo.reset();
        else if (data.type === "stop") this.failed = true;
      } catch (error) { this.failed = true; this.port.postMessage({type:"error", detail:error.message}); }
    };
  }
  process(_inputs, outputs) {
    if (this.failed) return false;
    const [left, right] = outputs[0];
    if (!left || !right) return true;
    this.fifo.fill(left, right); this.ticks += left.length;
    if (this.ticks >= 960) {
      this.ticks = 0;
      this.port.postMessage({type:"progress", consumed:this.fifo.consumed, queued:this.fifo.queued, buffering:this.fifo.buffering});
    }
    return true;
  }
}
registerProcessor("sda-remote-pcm", SdaRemotePcm);
