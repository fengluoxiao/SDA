// One bounded FIFO of interleaved f32 program samples. No gain, filters,
// interpolation, dropped frames or crossfades are applied here.
export class StereoPcmBuffer {
  constructor(capacity = 65536, refill = 1920) {
    this.capacity = capacity; this.refill = refill;
    this.samples = new Float32Array(capacity * 2);
    this.read = 0; this.write = 0; this.queued = 0; this.consumed = 0; this.buffering = true;
  }
  configure(bufferMs) {
    // Leave room for feedback latency within the sender's credit window.
    const ms=[100,300,600,1000].includes(bufferMs)?bufferMs:300;
    this.refill=Math.min(this.capacity,Math.max(1920,(ms-40)*48));
  }
  push(samples) {
    if (!(samples instanceof Float32Array) || samples.length % 2) throw Error("Invalid stereo PCM");
    const frames = samples.length / 2;
    if (frames > this.capacity - this.queued) throw Error("PCM buffer overflow");
    for (let i = 0; i < samples.length; i++) if (!Number.isFinite(samples[i])) throw Error("Invalid PCM sample");
    const first = Math.min(frames, this.capacity - this.write);
    this.samples.set(samples.subarray(0, first * 2), this.write * 2);
    if (first < frames) this.samples.set(samples.subarray(first * 2), 0);
    this.write = (this.write + frames) % this.capacity; this.queued += frames;
  }
  reset() {
    this.consumed += this.queued; this.queued = 0; this.read = this.write; this.buffering = true;
  }
  fill(left, right) {
    left.fill(0); right.fill(0);
    if (this.buffering && this.queued < this.refill) return 0;
    this.buffering = false;
    const count = Math.min(left.length, right.length, this.queued);
    for (let i = 0; i < count; i++) {
      const at = ((this.read + i) % this.capacity) * 2;
      left[i] = this.samples[at]; right[i] = this.samples[at + 1];
    }
    this.read = (this.read + count) % this.capacity; this.queued -= count; this.consumed += count;
    if (count < left.length) this.buffering = true;
    return count;
  }
}
