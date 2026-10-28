import type { DecodedFrameData, FrameLoudness, ObjectEvent } from "@sda/core";
import { VbapSolver } from "../../renderer/src/vbap";
import { admToSpherical } from "../../renderer/src/coords";
import { LoudnessMeter } from "./bs1770";
import { av3aStartupDecision } from "./av3a-startup";
/** Neutral speaker reference, never a raw-object channel sum or a listening render. */
export const ADM_REFERENCE_POLICY = "fixed-714-scene-startup-v1";
const angles = [[30, 0], [-30, 0], [0, 0], [0, 0], [135, 0], [-135, 0], [90, 0], [-90, 0], [45, 45], [-45, 45], [135, 45], [-135, 45]] as const;
const aliases = [
    ["L", "FL", "Left", "FrontLeft"], ["R", "FR", "Right", "FrontRight"], ["C", "FC", "Center", "FrontCenter"], ["LFE", "LFE1"],
    ["Lb", "BL", "BackLeft", "RearLeft"], ["Rb", "BR", "BackRight", "RearRight"], ["Ls", "SL", "SurroundLeft", "SideLeft"], ["Rs", "SR", "SurroundRight", "SideRight"],
    ["Tfl", "TopFrontLeft", "FrontHeightLeft"], ["Tfr", "TopFrontRight", "FrontHeightRight"], ["Tbl", "Trl", "TopRearLeft", "TopBackLeft", "RearHeightLeft"], ["Tbr", "Trr", "TopRearRight", "TopBackRight", "RearHeightRight"]
];
type Row = {
    current: Float32Array;
    target: Float32Array;
    remaining: number;
};
export class AdmProgrammeReference {
    private readonly solver = new VbapSolver(angles.map(([azimuth, elevation], i) => ({ name: String(i), azimuth, elevation, distance: 1, isLfe: i === 3 })));
    private readonly meter: LoudnessMeter;
    private readonly rows = new Map<number, Row>();
    private readonly states = new Map<number, ObjectEvent>();
    private readonly declarations = new Map<number, number>();
    private labels: string[] = [];
    private bedRows = new Map<string, Float32Array>();
    private readSamples = 0;
    private frozen = false;
    private value: FrameLoudness = { integratedLufs: null, blocks: 0, truePeakDbtp: null };
    constructor(readonly sampleRate: number) { this.meter = new LoudnessMeter(sampleRate, 12, [1, 1, 1, 0, 1.41, 1.41, 1.41, 1.41, 1, 1, 1, 1]); }
    get settled(): boolean { return this.frozen; }
    get measurement(): FrameLoudness { return { ...this.value }; }
    decision(ended = false) { return av3aStartupDecision(this.value, this.readSamples / this.sampleRate, ended); }
    private event(e: ObjectEvent): void {
        const previous = this.states.get(e.id);
        if (!e.hasPos && !previous)
            return;
        const state = e.hasPos ? e : { ...previous!, gainDb: e.gainDb, rampDuration: e.rampDuration, samplePos: e.samplePos };
        this.states.set(e.id, state);
        const pos = state.pos.some(x => Math.abs(x) > 1e-12) ? state.pos : [0, 1, 0] as [
            number,
            number,
            number
        ];
        const gains = this.solver.pan(admToSpherical(pos), Math.min(1, Math.max(0, state.diffuse ?? 0, ...state.size)));
        const gain = 10 ** (state.gainDb / 20);
        for (let i = 0; i < 12; i++)
            gains[i] = gains[i]! * gain;
        const row = this.rows.get(e.id);
        if (!row || state.rampDuration <= 1)
            this.rows.set(e.id, { current: gains.slice(), target: gains, remaining: 0 });
        else {
            row.target = gains;
            row.remaining = state.rampDuration;
        }
    }
    private mix(frame: DecodedFrameData, out: Float32Array[], start: number, end: number): void {
        for (let ch = 0; ch < frame.channels.length; ch++) {
            const input = frame.channels[ch]!, id = this.declarations.get(ch);
            if (id !== undefined) {
                const row = this.rows.get(id);
                if (!row)
                    continue;
                const ramp = Math.min(end - start, row.remaining);
                for (let bus = 0; bus < 12; bus++) {
                    const a = row.current[bus]!, b = row.target[bus]!;
                    if (a === 0 && b === 0)
                        continue;
                    const step = row.remaining ? (b - a) / row.remaining : 0;
                    for (let i = 0; i < ramp; i++)
                        out[bus]![start + i]! += input[start + i]! * (a + step * (i + 1));
                    const gain = ramp === row.remaining ? b : a + step * ramp;
                    for (let i = start + ramp; i < end; i++)
                        out[bus]![i]! += input[i]! * gain;
                    row.current[bus] = gain;
                }
                row.remaining -= ramp;
            }
            else {
                const label = frame.labels[ch]!, bus = aliases.findIndex(names => names.includes(label));
                const pair = ["Tsl", "TopMiddleLeft", "TopSideLeft"].includes(label) ? [8, 10] : ["Tsr", "TopMiddleRight", "TopSideRight"].includes(label) ? [9, 11] : null;
                if (bus >= 0) {
                    for (let i = start; i < end; i++)
                        out[bus]![i]! += input[i]!;
                }
                else if (pair) {
                    for (const target of pair)
                        for (let i = start; i < end; i++)
                            out[target]![i]! += input[i]! * Math.SQRT1_2;
                }
                else if (["Lw", "Rw", "WideLeft", "WideRight"].includes(label)) {
                    let gains = this.bedRows.get(label);
                    if (!gains) {
                        gains = this.solver.pan({ azimuth: label === "Lw" || label === "WideLeft" ? 60 : -60, elevation: 0, distance: 1 });
                        this.bedRows.set(label, gains);
                    }
                    for (let bus = 0; bus < 12; bus++)
                        if (gains[bus])
                            for (let i = start; i < end; i++)
                                out[bus]![i]! += input[i]! * gains[bus]!;
                }
                else if (input.subarray(start, end).some(x => x !== 0))
                    throw Error("Unsupported ADM reference bed: " + label);
            }
        }
    }
    measure(frame: DecodedFrameData): FrameLoudness {
        if (this.frozen)
            return this.measurement;
        if (frame.sampleRate !== this.sampleRate || frame.channels.some(c => c.length !== frame.channels[0]?.length))
            throw Error("Invalid ADM reference topology");
        const n = frame.channels[0]?.length ?? 0;
        if (!n)
            return this.measurement;
        if (frame.labels.join() !== this.labels.join()) {
            for (const [channel] of this.declarations)
                if (this.labels[channel] !== frame.labels[channel])
                    this.declarations.delete(channel);
            this.labels = [...frame.labels];
        }
        for (const d of frame.objectChannels) {
            for (const [old, id] of this.declarations)
                if (id === d.id && old !== d.channel)
                    this.declarations.delete(old);
            this.declarations.set(d.channel, d.id);
        }
        frame.labels.forEach((label, ch) => { if (/^Obj_\d+$/.test(label))
            this.declarations.set(ch, Number(label.slice(4))); });
        const active = new Set(this.declarations.values());
        for (const id of this.rows.keys())
            if (!active.has(id)) {
                this.rows.delete(id);
                this.states.delete(id);
            }
        const out = Array.from({ length: 12 }, () => new Float32Array(n));
        let offset = 0;
        for (const e of frame.events) {
            const at = Math.max(0, Math.min(n, e.samplePos - frame.samplePos));
            if (at < offset)
                throw Error("Unordered ADM reference events");
            this.mix(frame, out, offset, at);
            this.event(e);
            offset = at;
        }
        this.mix(frame, out, offset, n);
        for (let at = 0; at < n && !this.frozen;) {
            const take = Math.min(n - at, 1024 - this.readSamples % 1024, Math.max(0, Math.ceil(this.sampleRate * 7) - this.readSamples));
            if (!take) {
                this.frozen = true;
                break;
            }
            this.meter.push(out.map(c => c.subarray(at, at + take)));
            this.readSamples += take;
            at += take;
            this.value = this.meter.integrated();
            this.frozen = this.value.blocks >= 57 && this.value.integratedLufs !== null || this.readSamples >= this.sampleRate * 7;
        }
        return this.measurement;
    }
}
