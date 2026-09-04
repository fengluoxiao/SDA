/**
 * sda-renderer AudioWorkletProcessor — plain JS, no imports.
 *
 * PCM and metadata use the decoder's absolute sample clock. A late source may
 * produce silence for samples that have already passed, but it can never play
 * those stale samples later and drift away from the other object channels.
 */

const WORKLET_BUILD = "head-pose-route-v3";
const MAX_SOURCES = 64;
const CALLBACK_GAP_TELEMETRY_MS = 12;
const CALLBACK_GAP_ESCALATION_MS = 25;
const RING_SIZE = 1 << 18; // 262144 samples ≈ 5.5 s @48k per source
const RING_MASK = RING_SIZE - 1;

class SdaRendererProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const opts = (options && options.processorOptions) || {};
    this.busCount = opts.busCount || 12;
    this.paused = false;
    this.consumed = 0;
    this.lastTick = 0;
    this.epoch = Number.isSafeInteger(opts.epoch) ? opts.epoch : 0;
    this.timelineStarted = false;
    this.timelineOrigin = null;
    // While true, object metadata keeps sample-accurate scalar gain but its
    // canonical world-space route cannot overwrite the live head-relative route.
    this.headTracking = false;
    this.sources = new Map();
    this.underrunSamples = 0;
    this.rejectedBatches = 0;
    this.rejectedSources = 0;
    // 输出侧回调抖动测量：process() 两次调用的墙钟间隔应约等于一个
    // render quantum（128/48000≈2.67ms）。焦点切换/后台节流导致实时线程
    // 被延迟调度时这里会出现间隙——即便 PCM 环形缓冲充足也会听见卡顿。
    this.lastProcessAt = null;
    this.callbackGaps = 0;
    this.callbackGapsOver25Ms = 0;
    this.callbackGapMaxMs = 0;
    this.processDurationTotalMs = 0;
    this.processDurationCount = 0;
    this.processDurationMaxMs = 0;
    this.activityHoldSamples = Math.round((typeof sampleRate === "number" ? sampleRate : 48000) * 0.2);
    this.lastActiveBankMask = 0;
    this.port.postMessage({ type: "ready", ringSize: RING_SIZE, maxSources: MAX_SOURCES, build: WORKLET_BUILD });
    this.port.onmessage = (e) => this.onMessage(e.data);
  }

  createSource() {
    return {
      ring: new Float32Array(RING_SIZE),
      valid: new Uint8Array(RING_SIZE),
      validStart: Number.POSITIVE_INFINITY,
      validEnd: Number.NEGATIVE_INFINITY,
      gains: new Float32Array(this.busCount),
      target: new Float32Array(this.busCount),
      rampLeft: 0,
      rampStep: new Float32Array(this.busCount),
      /** Sorted bus indices whose current/target ramp can produce non-zero output. */
      routeBuses: [],
      gain: 1,
      targetGain: 1,
      gainStep: 0,
      gainRampLeft: 0,
      muteGain: 1,
      targetMuteGain: 1,
      muteRampLeft: 0,
      muteStep: 0,
      scheduledGains: [],
      scheduledGainCursor: 0,
      nextScheduledGainAt: Number.POSITIVE_INFINITY,
      lpA: 1,
      lpY: 0,
      binauralBank: 1,
      availabilityFrom: 0,
      availabilityRampLeft: 0,
      availabilityLastOutput: 0,
      availabilityWasValid: false,
      hasReceivedPcm: false,
      lifecycleEvents: [],
      lifecycleCursor: 0,
      lifecycleEventOrder: 0,
      nextLifecycleAt: Number.POSITIVE_INFINITY,
      futureResumeCount: 0,
      active: true,
      inactiveSince: null,
      inactiveToken: null,
      activityUntil: 0,
    };
  }

  refreshRouteBuses(src, includeRampTargets = false) {
    const routes = [];
    for (let bus = 0; bus < this.busCount; bus++) {
      if (
        src.gains[bus] !== 0 ||
        (includeRampTargets && (src.target[bus] !== 0 || src.rampStep[bus] !== 0))
      ) routes.push(bus);
    }
    src.routeBuses = routes;
  }

  /** Advance spatial and scalar ramps independently by exact sample intervals. */
  advanceGainRamps(src, samples, advanceSpatial = true, advanceScalar = true) {
    const count = Math.max(0, Math.trunc(samples));
    if (advanceSpatial) {
      const spatialAdvance = Math.min(count, src.rampLeft);
      if (spatialAdvance > 0) {
        for (let bus = 0; bus < this.busCount; bus++) {
          src.gains[bus] += src.rampStep[bus] * spatialAdvance;
        }
        src.rampLeft -= spatialAdvance;
        if (src.rampLeft === 0) {
          src.gains.set(src.target);
          this.refreshRouteBuses(src);
        }
      }
    }
    if (advanceScalar) {
      const scalarAdvance = Math.min(count, src.gainRampLeft);
      if (scalarAdvance > 0) {
        src.gain += src.gainStep * scalarAdvance;
        src.gainRampLeft -= scalarAdvance;
        if (src.gainRampLeft === 0) src.gain = src.targetGain;
      }
    }
  }

  /** Start an event at eventTime and fast-forward it to currentTime. */
  startGainRampAtTime(src, msg, eventTime, currentTime) {
    if (msg.type === "scheduleGains" && msg.poseUpdate === true && !this.headTracking) return;
    const ramp = Math.max(1, msg.ramp | 0);
    const preserveSpatial = this.headTracking
      && msg.type === "scheduleGains"
      && msg.poseControlled === true
      && msg.poseUpdate !== true;
    const preserveScalar = msg.poseUpdate === true;
    if (!preserveSpatial) {
      const target = msg.gains;
      for (let bus = 0; bus < this.busCount; bus++) {
        src.target[bus] = Math.min(target.length > bus ? target[bus] : 0, 4);
        src.rampStep[bus] = (src.target[bus] - src.gains[bus]) / ramp;
      }
      src.rampLeft = ramp;
      this.refreshRouteBuses(src, true);
    }
    if (!preserveScalar) {
      src.targetGain = msg.gain ?? 1;
      src.gainStep = (src.targetGain - src.gain) / ramp;
      src.gainRampLeft = ramp;
      src.lpA = typeof msg.lp === "number" ? Math.min(1, Math.max(0, msg.lp)) : 1;
    }
    this.advanceGainRamps(
      src,
      currentTime - eventTime,
      !preserveSpatial,
      !preserveScalar,
    );
  }

  /** Replay every overdue event chronologically to currentTime. Each event is
   * advanced only as far as the next event that interrupts it (or now). */
  applyScheduledGainsThrough(src, currentTime) {
    const events = src.scheduledGains;
    let cursor = src.scheduledGainCursor;
    while (cursor < events.length && events[cursor].at <= currentTime) {
      const msg = events[cursor++];
      const next = events[cursor];
      const replayThrough = next && next.at <= currentTime ? next.at : currentTime;
      this.startGainRampAtTime(src, msg, msg.at, replayThrough);
    }
    src.scheduledGainCursor = cursor;
    if (cursor >= 256 && cursor * 2 >= events.length) {
      events.splice(0, cursor);
      src.scheduledGainCursor = 0;
      cursor = 0;
    }
    src.nextScheduledGainAt = events[cursor]?.at ?? Number.POSITIVE_INFINITY;
  }

  enqueueScheduledGain(src, msg) {
    const events = src.scheduledGains;
    const cursor = src.scheduledGainCursor;
    if (msg.poseUpdate === true) {
      // Pose refreshes repeatedly update already-buffered object boundaries.
      // Replace the pending route instead of growing the queue at 120 Hz.
      for (let index = cursor; index < events.length; index++) {
        const candidate = events[index];
        if (candidate.at > msg.at) break;
        if (candidate.at === msg.at && candidate.poseUpdate === true) {
          events[index] = msg;
          src.nextScheduledGainAt = events[cursor]?.at ?? Number.POSITIVE_INFINITY;
          return;
        }
      }
    }
    const last = events[events.length - 1];
    if (!last || last.at <= msg.at) {
      events.push(msg);
      src.nextScheduledGainAt = events[cursor]?.at ?? Number.POSITIVE_INFINITY;
      return;
    }
    let low = cursor;
    let high = events.length;
    while (low < high) {
      const middle = (low + high) >>> 1;
      if (events[middle].at <= msg.at) low = middle + 1;
      else high = middle;
    }
    events.splice(low, 0, msg);
    src.nextScheduledGainAt = events[cursor]?.at ?? Number.POSITIVE_INFINITY;
  }

  discardScheduledPoseUpdates(src) {
    src.scheduledGains = src.scheduledGains
      .slice(src.scheduledGainCursor)
      .filter((event) => event.poseUpdate !== true);
    src.scheduledGainCursor = 0;
    src.nextScheduledGainAt = src.scheduledGains[0]?.at ?? Number.POSITIVE_INFINITY;
  }

  scheduleLifecycle(src, at, active, token = null) {
    if (!Number.isSafeInteger(at)) return;
    const event = { at, active, token, order: src.lifecycleEventOrder++ };
    const events = src.lifecycleEvents;
    let low = src.lifecycleCursor;
    let high = events.length;
    while (low < high) {
      const middle = (low + high) >>> 1;
      const candidate = events[middle];
      if (candidate.at < at || (candidate.at === at && candidate.order <= event.order)) low = middle + 1;
      else high = middle;
    }
    events.splice(low, 0, event);
    if (active) src.futureResumeCount++;
    src.nextLifecycleAt = events[src.lifecycleCursor]?.at ?? Number.POSITIVE_INFINITY;
  }

  applyLifecycleThrough(src, currentTime) {
    const events = src.lifecycleEvents;
    let cursor = src.lifecycleCursor;
    while (cursor < events.length && events[cursor].at <= currentTime) {
      const event = events[cursor++];
      if (event.active) src.futureResumeCount--;
      src.active = event.active;
      src.inactiveSince = event.active ? null : event.at;
      src.inactiveToken = event.active ? null : event.token;
    }
    src.lifecycleCursor = cursor;
    if (cursor >= 64 && cursor * 2 >= events.length) {
      events.splice(0, cursor);
      src.lifecycleCursor = 0;
      cursor = 0;
    }
    src.nextLifecycleAt = events[cursor]?.at ?? Number.POSITIVE_INFINITY;
  }

  rejectBatch(sequence, reason) {
    this.rejectedBatches++;
    this.port.postMessage({ type: "batchRejected", sequence, reason });
  }

  feedBatch(start, entries, sequence) {
    if (!Number.isSafeInteger(start) || entries.length === 0) {
      this.rejectBatch(sequence, "invalid");
      return;
    }
    const sources = entries.map((entry) => this.sources.get(entry.id));
    // Port messages are FIFO, so adds normally precede the batch. Never accept
    // only part of a frame if a declaration is missing: partial writes would
    // destroy inter-object phase and time alignment.
    if (sources.some((source) => !source)) {
      this.rejectBatch(sequence, "missing-source");
      return;
    }

    const inputLength = entries.reduce(
      (length, entry) => Math.min(length, entry.samples.length),
      Number.POSITIVE_INFINITY,
    );
    const skipped = this.timelineStarted
      ? Math.min(inputLength, Math.max(0, this.consumed - start))
      : 0;
    const writeStart = start + skipped;
    if (!this.timelineStarted && !Number.isFinite(this.timelineOrigin)) this.timelineOrigin = writeStart;
    const ahead = this.timelineStarted ? Math.max(0, writeStart - this.consumed) : writeStart - this.timelineOrigin;
    const count = inputLength - skipped;
    if (count <= 0 || ahead < 0 || ahead + count > RING_SIZE) {
      this.rejectBatch(sequence, count <= 0 ? "late" : "ring-full");
      return;
    }

    for (let entryIndex = 0; entryIndex < entries.length; entryIndex++) {
      const source = sources[entryIndex];
      const samples = entries[entryIndex].samples;
      // Materialise an explicit silence gap on timeline discontinuities so old
      // ring contents can never be mistaken for valid PCM.
      if (Number.isFinite(source.validEnd) && writeStart > source.validEnd) {
        const gapEnd = Math.min(writeStart, source.validEnd + RING_SIZE);
        for (let position = source.validEnd; position < gapEnd; position++) {
          const slot = position & RING_MASK;
          source.ring[slot] = 0;
          source.valid[slot] = 0;
        }
      }
      for (let i = 0; i < count; i++) {
        const slot = (writeStart + i) & RING_MASK;
        source.ring[slot] = samples[skipped + i];
        source.valid[slot] = 1;
      }
      source.validStart = Number.isFinite(source.validStart)
        ? Math.min(source.validStart, writeStart)
        : writeStart;
      source.validEnd = Number.isFinite(source.validEnd)
        ? Math.max(source.validEnd, writeStart + count)
        : writeStart + count;
      source.hasReceivedPcm = true;
    }
    this.port.postMessage({ type: "batchAck", sequence, samples: count });
  }

  onMessage(msg) {
    switch (msg.type) {
      case "add":
        if (this.sources.size >= MAX_SOURCES && !this.sources.has(msg.id)) {
          this.rejectedSources++;
          this.port.postMessage({ type: "sourceRejected", id: msg.id, maxSources: MAX_SOURCES });
        } else if (!this.sources.has(msg.id)) {
          this.sources.set(msg.id, this.createSource());
        }
        break;
      case "remove":
        this.sources.delete(msg.id);
        break;
      case "removeAt": {
        const src = this.sources.get(msg.id);
        if (src) this.scheduleLifecycle(src, msg.at, false, msg.token);
        break;
      }
      case "resumeAt": {
        const src = this.sources.get(msg.id);
        if (src) this.scheduleLifecycle(src, msg.at, true);
        break;
      }
      case "feed": {
        const src = this.sources.get(msg.id);
        if (!src) break;
        const start = Number.isFinite(src.validEnd) ? src.validEnd : this.consumed;
        this.feedBatch(start, [{ id: msg.id, samples: msg.samples }], -1);
        break;
      }
      case "feedBatch":
        this.feedBatch(msg.start, msg.entries || [], msg.sequence);
        break;
      case "gains": {
        const src = this.sources.get(msg.id);
        if (src) {
          this.startGainRampAtTime(src, msg, this.consumed, this.consumed);
        }
        break;
      }
      case "scheduleGains": {
        const src = this.sources.get(msg.id);
        if (!src || !Number.isSafeInteger(msg.at)) break;
        this.enqueueScheduledGain(src, msg);
        break;
      }
      case "scheduleGainsBatch":
        for (const entry of msg.entries || []) {
          const src = this.sources.get(entry.id);
          if (src && Number.isSafeInteger(entry.at)) this.enqueueScheduledGain(src, entry);
        }
        break;
      case "headTracking":
        if (this.headTracking !== (msg.enabled === true)) {
          this.headTracking = msg.enabled === true;
          for (const src of this.sources.values()) this.discardScheduledPoseUpdates(src);
        }
        break;
      case "mute": {
        const src = this.sources.get(msg.id);
        if (!src) break;
        const ramp = Math.max(1, msg.ramp | 0);
        src.targetMuteGain = msg.muted ? 0 : 1;
        src.muteStep = (src.targetMuteGain - src.muteGain) / ramp;
        src.muteRampLeft = ramp;
        break;
      }
      case "binauralMode": {
        const src = this.sources.get(msg.id);
        if (src) src.binauralBank = Math.max(0, Math.min(3, msg.bank | 0));
        break;
      }
      case "start":
        if (!this.timelineStarted && Number.isSafeInteger(msg.origin)) {
          this.consumed = msg.origin;
          this.lastTick = msg.origin;
          this.timelineOrigin = msg.origin;
          this.timelineStarted = true;
        }
        break;
      case "reset":
        for (const src of this.sources.values()) {
          src.validStart = Number.POSITIVE_INFINITY;
          src.validEnd = Number.NEGATIVE_INFINITY;
          src.valid.fill(0);
          src.scheduledGains.length = 0;
          src.scheduledGainCursor = 0;
          src.nextScheduledGainAt = Number.POSITIVE_INFINITY;
          this.refreshRouteBuses(src, src.rampLeft > 0);
          src.lpY = 0;
          src.availabilityFrom = 0;
          src.availabilityRampLeft = 0;
          src.availabilityLastOutput = 0;
          src.availabilityWasValid = false;
          src.hasReceivedPcm = false;
          src.lifecycleEvents.length = 0;
          src.lifecycleCursor = 0;
          src.lifecycleEventOrder = 0;
          src.nextLifecycleAt = Number.POSITIVE_INFINITY;
          src.futureResumeCount = 0;
          src.active = true;
          src.inactiveSince = null;
          src.inactiveToken = null;
        }
        this.paused = false;
        this.consumed = 0;
        this.lastTick = 0;
        this.epoch = Number.isSafeInteger(msg.epoch) ? msg.epoch : this.epoch + 1;
        this.timelineStarted = false;
        this.timelineOrigin = null;
        this.underrunSamples = 0;
        this.rejectedBatches = 0;
        this.rejectedSources = 0;
        this.callbackGaps = 0;
        this.callbackGapsOver25Ms = 0;
        this.callbackGapMaxMs = 0;
        this.processDurationTotalMs = 0;
        this.processDurationCount = 0;
        this.processDurationMaxMs = 0;
        this.lastProcessAt = null;
        this.lastActiveBankMask = 0;
        this.port.postMessage({ type: "resetAck", epoch: this.epoch });
        break;
      case "pause":
        this.paused = !!msg.paused;
        break;
    }
  }

  buffered(id) {
    const src = this.sources.get(id);
    return src && Number.isFinite(src.validEnd)
      ? Math.max(0, src.validEnd - this.consumed)
      : 0;
  }

  wallClockMs() {
    const precise = (typeof performance !== "undefined" && performance.now) ? performance.now() : 0;
    // Some Electron AudioWorklet globals expose performance.now() but permanently
    // return zero. Date.now() is lower resolution, but remains a usable wall clock
    // for cross-machine callback-gap telemetry in that environment.
    return precise > 0 ? precise : Date.now();
  }

  recordProcessDuration(startedAt) {
    const endedAt = this.wallClockMs();
    if (endedAt < startedAt) return;
    const durationMs = endedAt - startedAt;
    this.processDurationTotalMs += durationMs;
    this.processDurationCount++;
    if (durationMs > this.processDurationMaxMs) this.processDurationMaxMs = durationMs;
  }

  process(_inputs, outputs) {
    const now = this.wallClockMs();
    const processStartedAt = now;
    if (this.lastProcessAt !== null && this.timelineStarted && !this.paused) {
      const gapMs = now - this.lastProcessAt;
      // Normal spacing is about 2.67ms. Keep broad >12ms telemetry for
      // diagnostics, while the adaptive output FIFO uses only >25ms evidence.
      if (gapMs > CALLBACK_GAP_TELEMETRY_MS) {
        this.callbackGaps++;
        if (gapMs > this.callbackGapMaxMs) this.callbackGapMaxMs = gapMs;
      }
      if (gapMs > CALLBACK_GAP_ESCALATION_MS) this.callbackGapsOver25Ms++;
    }
    this.lastProcessAt = now;
    const busesByBank = outputs;
    const primaryBuses = busesByBank[0] || [];
    const blockSize = primaryBuses[0] ? primaryBuses[0].length : 128;
    let activeBankMask = 0;
    for (const src of this.sources.values()) {
      if (src.active) activeBankMask |= 1 << src.binauralBank;
    }
    // Explicitly clear active banks plus a just-retired bank for one final block.
    // AudioWorklet outputs are normally zeroed by the host; this preserves that
    // guarantee without filling all four 18-channel banks every quantum.
    const banksToClear = activeBankMask | this.lastActiveBankMask;
    for (let bank = 0; bank < busesByBank.length; bank++) {
      if ((banksToClear & (1 << bank)) === 0) continue;
      const buses = busesByBank[bank];
      for (let bus = 0; bus < this.busCount && bus < buses.length; bus++) buses[bus].fill(0);
    }
    this.lastActiveBankMask = activeBankMask;
    if (this.paused || !this.timelineStarted) {
      this.recordProcessDuration(processStartedAt);
      return true;
    }

    for (const [sourceId, src] of this.sources) {
      const buses = busesByBank[src.binauralBank] || primaryBuses;
      let gain = src.gain;
      let muteGain = src.muteGain;
      let lpY = src.lpY;
      let activityUntil = src.activityUntil;

      for (let i = 0; i < blockSize; i++) {
        const samplePosition = this.consumed + i;
        // Commit this block's local scalar before an event replaces the ramp.
        // Otherwise an event in the middle of a render quantum can jump back to
        // src.gain, which still holds the value from the block boundary.
        src.gain = gain;
        if (samplePosition >= src.nextScheduledGainAt) this.applyScheduledGainsThrough(src, samplePosition);
        gain = src.gain;

        let sample = 0;
        const slot = samplePosition & RING_MASK;
        if (samplePosition >= src.nextLifecycleAt) this.applyLifecycleThrough(src, samplePosition);
        const retired = !src.active;
        const available = src.active && samplePosition >= src.validStart && samplePosition < src.validEnd && src.valid[slot] === 1;
        if (available !== src.availabilityWasValid) {
          src.availabilityWasValid = available;
          src.availabilityFrom = src.availabilityLastOutput;
          src.availabilityRampLeft = 32;
        }
        if (!retired && !available && src.hasReceivedPcm && samplePosition >= src.validStart) this.underrunSamples++;
        const target = available ? src.ring[slot] : 0;
        if (src.availabilityRampLeft > 0) {
          const progress = (33 - src.availabilityRampLeft) / 32;
          sample = src.availabilityFrom + (target - src.availabilityFrom) * progress;
          src.availabilityRampLeft--;
        } else {
          sample = target;
        }
        src.availabilityLastOutput = sample;
        if (src.lpA < 0.999) {
          lpY += src.lpA * (sample - lpY);
          sample = lpY;
        }
        sample *= gain * muteGain;
        // UI activity reflects the actual source sample after metadata gain and user mute.
        // A short hold makes intermittent drum hits readable at the throttled visual cadence.
        if (Math.abs(sample) >= 0.001) activityUntil = samplePosition + this.activityHoldSamples;

        for (const bus of src.routeBuses) {
          if (bus >= buses.length) break;
          buses[bus][i] += sample * src.gains[bus];
        }

        if (src.rampLeft > 0 || src.gainRampLeft > 0) {
          this.advanceGainRamps(src, 1);
          gain = src.gain;
        }
        if (src.muteRampLeft > 0) {
          muteGain += src.muteStep;
          src.muteRampLeft--;
          if (src.muteRampLeft === 0) muteGain = src.targetMuteGain;
        }
      }
      src.gain = gain;
      src.muteGain = muteGain;
      src.lpY = lpY;
      src.activityUntil = activityUntil;
      const blockEnd = this.consumed + blockSize;
      if (!src.active && src.futureResumeCount === 0 && src.inactiveSince !== null && blockEnd >= src.inactiveSince + 32) {
        this.sources.delete(sourceId);
        this.port.postMessage({ type: "sourceRetired", id: sourceId, token: src.inactiveToken });
      }
    }

    this.consumed += blockSize;
    this.recordProcessDuration(processStartedAt);
    const tickEvery = (typeof sampleRate === "number" ? sampleRate : 48000) >> 3;
    if (this.consumed - this.lastTick >= tickEvery) {
      this.lastTick = this.consumed;
      const activeObjectIds = [];
      for (const [sourceId, src] of this.sources) {
        if (sourceId.startsWith("obj:") && src.active && this.consumed <= src.activityUntil) {
          activeObjectIds.push(sourceId.slice(4));
        }
      }
      this.port.postMessage({
        type: "tick",
        consumed: this.consumed,
        epoch: this.epoch,
        underrunSamples: this.underrunSamples,
        rejectedBatches: this.rejectedBatches,
        rejectedSources: this.rejectedSources,
        callbackGaps: this.callbackGaps,
        callbackGapsOver25Ms: this.callbackGapsOver25Ms,
        callbackGapMaxMs: Math.round(this.callbackGapMaxMs * 10) / 10,
        processMeanMs: this.processDurationCount > 0
          ? Math.round(this.processDurationTotalMs / this.processDurationCount * 1000) / 1000
          : 0,
        processMaxMs: Math.round(this.processDurationMaxMs * 1000) / 1000,
        activeObjectIds,
      });
      this.underrunSamples = 0;
      this.rejectedBatches = 0;
      this.rejectedSources = 0;
      this.callbackGaps = 0;
      this.callbackGapsOver25Ms = 0;
      this.callbackGapMaxMs = 0;
      this.processDurationTotalMs = 0;
      this.processDurationCount = 0;
      this.processDurationMaxMs = 0;
    }
    return true;
  }
}

/** Stereo-linked lookahead limiter. Both ears share one gain envelope so peak
 * control cannot shift the binaural image. The short release prevents one sparse
 * object transient from suppressing the following object-update interval. */
class SdaFinalPeakGuardProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const ceilingDb = options?.processorOptions?.ceilingDb ?? -1;
    this.ceiling = Math.pow(10, ceilingDb / 20);
    this.lookahead = Math.max(1, Math.round((typeof sampleRate === "number" ? sampleRate : 48000) * 0.005));
    this.releaseCoeff = Math.exp(-1 / ((typeof sampleRate === "number" ? sampleRate : 48000) * 0.02));
    this.buffers = [new Float32Array(this.lookahead), new Float32Array(this.lookahead)];
    this.write = 0;
    this.gain = 1;
    this.attackTarget = 1;
    this.attackStep = 0;
    this.hold = 0;
    this.timelineStarted = false;
    this.paused = false;
    this.consumed = 0;
    this.programEnabled = false;
    this.programMetadataGain = 1;
    this.programGain = 1;
    this.programTargetGain = 1;
    this.programGainStep = 0;
    this.programRampLeft = 0;
    this.scheduledProgramGains = [];
    this.programEventOrder = 0;
    this.port.onmessage = (event) => this.onMessage(event.data);
  }

  setProgramTarget(target, ramp) {
    this.programTargetGain = this.programEnabled ? target : 1;
    if (!this.timelineStarted) {
      this.programGain = this.programTargetGain;
      this.programGainStep = 0;
      this.programRampLeft = 0;
      return;
    }
    this.programRampLeft = Math.max(1, ramp | 0);
    this.programGainStep = (this.programTargetGain - this.programGain) / this.programRampLeft;
  }

  normalizeProgramGain(value) {
    const gain = Number(value);
    return Number.isFinite(gain) ? Math.max(0, Math.min(1, gain)) : 1;
  }

  onMessage(msg) {
    const ramp = Math.max(1, Math.round((typeof sampleRate === "number" ? sampleRate : 48000) * 0.05));
    switch (msg.type) {
      case "programGain":
        this.programMetadataGain = this.normalizeProgramGain(msg.gain);
        this.setProgramTarget(this.programMetadataGain, ramp);
        break;
      case "scheduleProgramGain":
        if (!Number.isSafeInteger(msg.at)) break;
        this.scheduledProgramGains.push({
          at: msg.at,
          gain: this.normalizeProgramGain(msg.gain),
          order: this.programEventOrder++,
        });
        this.scheduledProgramGains.sort((left, right) => left.at - right.at || left.order - right.order);
        break;
      case "programEnabled":
        this.programEnabled = !!msg.enabled;
        this.setProgramTarget(this.programMetadataGain, ramp);
        break;
      case "start":
        if (!this.timelineStarted && Number.isSafeInteger(msg.origin)) {
          this.consumed = msg.origin;
          this.applyProgramEventsThrough(msg.origin);
          this.timelineStarted = true;
        }
        break;
      case "reset":
        this.timelineStarted = false;
        this.paused = false;
        this.consumed = 0;
        for (const buffer of this.buffers) buffer.fill(0);
        this.write = 0;
        this.gain = 1;
        this.attackTarget = 1;
        this.attackStep = 0;
        this.hold = 0;
        this.programMetadataGain = 1;
        this.programGain = 1;
        this.programTargetGain = 1;
        this.programGainStep = 0;
        this.programRampLeft = 0;
        this.scheduledProgramGains.length = 0;
        this.programEventOrder = 0;
        break;
      case "pause":
        this.paused = !!msg.paused;
        break;
    }
  }

  applyProgramEventsThrough(samplePosition) {
    while (this.scheduledProgramGains.length > 0 && this.scheduledProgramGains[0].at <= samplePosition) {
      const event = this.scheduledProgramGains.shift();
      this.programMetadataGain = event.gain;
      const ramp = Math.max(1, Math.round((typeof sampleRate === "number" ? sampleRate : 48000) * 0.05));
      this.setProgramTarget(event.gain, ramp);
    }
  }

  process(inputs, outputs) {
    const input = inputs[0] || [];
    const output = outputs[0] || [];
    const blockSize = output[0]?.length ?? 128;
    if (this.paused) {
      for (const channel of output) channel.fill(0);
      return true;
    }
    for (let i = 0; i < blockSize; i++) {
      if (this.timelineStarted) this.applyProgramEventsThrough(this.consumed + i);
      let peak = 0;
      const delayedLeft = this.buffers[0][this.write];
      const delayedRight = this.buffers[1][this.write];
      for (let channel = 0; channel < 2; channel++) {
        const source = input[channel] || input[0];
        const raw = Number.isFinite(source?.[i]) ? source[i] : 0;
        const sample = raw * this.programGain;
        this.buffers[channel][this.write] = sample;
        peak = Math.max(peak, Math.abs(sample));
      }
      const target = peak > this.ceiling ? this.ceiling / peak : 1;
      if (target < this.attackTarget) {
        const nextStep = (target - this.gain) / this.lookahead;
        this.attackStep = this.gain > this.attackTarget
          ? Math.min(this.attackStep, nextStep)
          : nextStep;
        this.attackTarget = target;
      }
      if (target < 1) this.hold = this.lookahead;
      if (this.gain > this.attackTarget) {
        this.gain = Math.max(this.attackTarget, this.gain + this.attackStep);
        if (this.gain === this.attackTarget) this.attackStep = 0;
      } else if (this.hold > 0) {
        this.hold--;
      } else {
        this.gain = 1 - (1 - this.gain) * this.releaseCoeff;
        this.attackTarget = this.gain;
      }
      for (let channel = 0; channel < output.length; channel++) {
        const delayed = channel === 0 ? delayedLeft : delayedRight;
        output[channel][i] = Math.max(-1, Math.min(1, delayed * this.gain));
      }
      if (this.programRampLeft > 0) {
        this.programGain += this.programGainStep;
        this.programRampLeft--;
        if (this.programRampLeft === 0) this.programGain = this.programTargetGain;
      }
      this.write = (this.write + 1) % this.lookahead;
    }
    if (this.timelineStarted) this.consumed += blockSize;
    return true;
  }
}

registerProcessor("sda-renderer", SdaRendererProcessor);
registerProcessor("sda-final-peak-guard", SdaFinalPeakGuardProcessor);
