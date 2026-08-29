/**
 * Incremental ITU-R BS.1770-4 integrated loudness (LKFS/LUFS).
 *
 * K-weighting follows the libebur128 analytic design of the two-stage
 * filter (high shelf + RLB high-pass), valid at arbitrary sample rates.
 * Blocks are 400 ms with a 100 ms hop; gating is the standard two-stage
 * absolute (−70 LUFS) / relative (ungated mean − 10 LU) gate.
 */

/** Channel weights per BS.1770-4 for the layouts this player can see. */
function channelWeights(count: number): readonly number[] {
  if (count <= 2) return new Array<number>(count).fill(1);
  // 5.1-style ordering (L R C LFE Ls Rs); LFE carries no loudness weight.
  const weights = new Array<number>(count).fill(1.41);
  weights[0] = 1;
  weights[1] = 1;
  if (count >= 4) weights[3] = 0;
  return weights;
}

class Biquad {
  private x1 = 0;
  private x2 = 0;
  private y1 = 0;
  private y2 = 0;
  constructor(private b0: number, private b1: number, private b2: number, private a1: number, private a2: number) {}

  process(input: Float32Array, output: Float32Array): void {
    const { b0, b1, b2, a1, a2 } = this;
    let x1 = this.x1;
    let x2 = this.x2;
    let y1 = this.y1;
    let y2 = this.y2;
    for (let i = 0; i < input.length; i++) {
      const x0 = input[i] ?? 0;
      const y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
      x2 = x1;
      x1 = x0;
      y2 = y1;
      y1 = y0;
      output[i] = y0;
    }
    this.x1 = x1;
    this.x2 = x2;
    this.y1 = y1;
    this.y2 = y2;
  }
}

/** libebur128 stage 1: high shelf (+4 dB toward Nyquist). */
function highShelf(sampleRate: number): Biquad {
  const f0 = 1681.974450955533;
  const gain = 3.999843853973347;
  const q = 0.7071752369554196;
  const k = Math.tan((Math.PI * f0) / sampleRate);
  const vh = Math.pow(10, gain / 20);
  const vb = Math.pow(vh, 0.4996667741545416);
  const a0 = 1 + k / q + k * k;
  return new Biquad(
    (vh + vb * k / q + k * k) / a0,
    (2 * (k * k - vh)) / a0,
    (vh - vb * k / q + k * k) / a0,
    (2 * (k * k - 1)) / a0,
    (1 - k / q + k * k) / a0,
  );
}

/** libebur128 stage 2: RLB high-pass (~38 Hz). */
function rlbHighPass(sampleRate: number): Biquad {
  const f0 = 38.13547087602444;
  const q = 0.5003270373238773;
  const k = Math.tan((Math.PI * f0) / sampleRate);
  const a0 = 1 + k / q + k * k;
  return new Biquad(1, -2, 1, (2 * (k * k - 1)) / a0, (1 - k / q + k * k) / a0);
}

export interface IntegratedLoudness {
  /** Gated integrated loudness in LUFS, or null while nothing passes the absolute gate. */
  integratedLufs: number | null;
  /** Completed 400 ms blocks above the absolute gate. */
  blocks: number;
}

export class LoudnessMeter {
  private readonly weights: readonly number[];
  private readonly filters: readonly [Biquad, Biquad][];
  private readonly blockSamples: number;
  private readonly hopSamples: number;
  /** Ring accumulator for the current 400 ms block, per channel. */
  private readonly block: Float32Array[];
  private readonly scratch: Float32Array;
  private fill = 0;
  /** Mean-square energy per completed 400 ms block (sum over weighted channels). */
  private readonly blockEnergy: number[] = [];

  constructor(sampleRate: number, channelCount: number) {
    this.weights = channelWeights(channelCount);
    this.filters = Array.from({ length: channelCount }, () => [highShelf(sampleRate), rlbHighPass(sampleRate)] as const);
    this.blockSamples = Math.round(sampleRate * 0.4);
    this.hopSamples = Math.round(sampleRate * 0.1);
    this.block = Array.from({ length: channelCount }, () => new Float32Array(this.blockSamples));
    this.scratch = new Float32Array(this.blockSamples);
  }

  /** Feed one frame of planar audio; all channels must share one length. */
  push(channels: readonly Float32Array[]): void {
    const first = channels[0];
    if (!first?.length) return;
    for (let offset = 0; offset < first.length;) {
      const take = Math.min(this.blockSamples - this.fill, first.length - offset);
      for (let ch = 0; ch < this.filters.length; ch++) {
        const input = channels[ch];
        const work = this.block[ch];
        const stages = this.filters[ch];
        if (!input || !work || !stages) continue;
        // Stage 1 into shared scratch, stage 2 appended at the open block tail.
        stages[0].process(input.subarray(offset, offset + take), this.scratch.subarray(0, take));
        stages[1].process(this.scratch.subarray(0, take), work.subarray(this.fill, this.fill + take));
      }
      offset += take;
      this.fill += take;
      if (this.fill < this.blockSamples) break;
      this.completeBlock();
      // The next block starts one hop later: drop the oldest 100 ms and keep
      // the freshest 300 ms of overlap.
      for (const work of this.block) work.copyWithin(0, this.hopSamples, this.blockSamples);
      this.fill = this.blockSamples - this.hopSamples;
    }
  }

  private completeBlock(): void {
    let energy = 0;
    for (let ch = 0; ch < this.filters.length; ch++) {
      const work = this.block[ch];
      if (!work) continue;
      let sum = 0;
      for (const sample of work) sum += sample * sample;
      energy += (this.weights[ch] ?? 1) * (sum / this.blockSamples);
    }
    this.blockEnergy.push(energy);
  }

  integrated(): IntegratedLoudness {
    const aboveAbsolute = this.blockEnergy.filter((energy) => energy > 0 && 10 * Math.log10(energy) > -69.309);
    if (aboveAbsolute.length === 0) return { integratedLufs: null, blocks: 0 };
    const ungatedMean = aboveAbsolute.reduce((a, b) => a + b, 0) / aboveAbsolute.length;
    const relativeGateLufs = -0.691 + 10 * Math.log10(ungatedMean) - 10;
    const aboveRelative = aboveAbsolute.filter(
      (energy) => -0.691 + 10 * Math.log10(energy) > relativeGateLufs,
    );
    const gatedMean = aboveRelative.reduce((a, b) => a + b, 0) / aboveRelative.length;
    return { integratedLufs: -0.691 + 10 * Math.log10(gatedMean), blocks: aboveAbsolute.length };
  }
}
