/** Preserve independent ADM PCM lanes and sample-clock automation; never render a bed downmix. */
import fs from "node:fs";
import { createHash } from "node:crypto";
import { deflateSync, createDeflate } from "node:zlib";
import { Readable } from "node:stream";
import { readBwfMetadata } from "@sda/demux";
import { exportRenderMetadata } from "./export-scene";
import { readSourceMetadata } from "./source-metadata";
const { buildMetadataChunks, chunk } = require("../../../apps/desktop/wav-metadata.cjs");
const roles: Record<string, string> = { L: "FrontLeft", R: "FrontRight", C: "Center", LFE: "LFE", Ls: "SurroundLeft", Rs: "SurroundRight", Lb: "RearLeft", Rb: "RearRight", Lw: "WideLeft", Rw: "WideRight", Tfl: "TopFrontLeft", Tfr: "TopFrontRight", Tbl: "TopRearLeft", Tbr: "TopRearRight", Tsl: "TopMiddleLeft", Tsr: "TopMiddleRight" };
const spherical = (p: readonly number[]) => { const distance = Math.hypot(...p); return { azimuth: Math.atan2(-p[0]!, p[1]!) * 180 / Math.PI, elevation: distance ? Math.asin(p[2]! / distance) * 180 / Math.PI : 0, distance }; };
export async function exportAdmMaster(job: any, check: () => void, progress: (stage: string, value: number, force?: boolean) => void) {
    const input = fs.openSync(job.inputPath, "r");
    let output: number | undefined;
    try {
        const initial = fs.fstatSync(input), size = initial.size;
        const read = async (at: number, n: number) => { check(); const b = Buffer.alloc(n); let k = 0; while (k < n) {
            const got = fs.readSync(input, b, k, n - k, at + k);
            if (!got)
                throw Error("ADM 母版截断");
            k += got;
        } return b; };
        const header = await read(0, 12);
        if (!["RIFF", "RF64", "BW64"].includes(header.toString("ascii", 0, 4)) || header.toString("ascii", 8, 12) !== "WAVE")
            return null;
        // Probe metadata headers, not channel count: mono/stereo ADM may also carry objects.
        let at = 12, hasAdm = false, dsData: number | undefined;
        while (at + 8 <= size) {
            const h = await read(at, 8), id = h.toString("ascii", 0, 4);
            let n = h.readUInt32LE(4);
            if (id === "ds64")
                dsData = Number((await read(at + 8, 28)).readBigUInt64LE(8));
            if (n === 0xffffffff) {
                if (id === "data" && dsData !== undefined)
                    n = dsData;
                else {
                    hasAdm = true;
                    break;
                }
            }
            if (id === "axml")
                hasAdm = true;
            at += 8 + n + (n & 1);
        }
        if (!hasAdm)
            return null;
        progress("metadata", 0, true);
        const bwf = await readBwfMetadata(read, size), adm = bwf.adm;
        if (!adm?.objectChannels.length)
            throw Error("多声道母版缺少可导出的 ADM 对象，不能按立体声上混处理");
        if (bwf.format.sampleRate !== 48000 || bwf.format.bits !== 24 || ![1, 65534].includes(bwf.format.encoding) || bwf.format.channels > 128)
            throw Error("ADM 对象母版需要 48 kHz / PCM24，最多 128 个独立信号");
        const frames = bwf.dataSize / bwf.format.blockAlign;
        if (adm.events.some(e => (e.zoneExclusion?.length ?? 0) > 0 || e.horizontalOnly))
            throw Error("此版本的 SDA 对象交付不支持 zoneExclusion / horizontalOnly，不能忽略这些必需参数");
        const source = await readSourceMetadata(job.inputPath, check);
        if (!source.common.title) {
            source.common.title = adm.title ?? "SDA Object Master";
            (source.source.common as Record<string, unknown>).title = source.common.title;
        }
        const add = (data: Buffer, filename: string, mimeType: string) => { const sha256 = createHash("sha256").update(data).digest("hex"); source.blobs.push({ sha256, data, filename, mimeType, kind: "render" }); return sha256; };
        const assets = job.renderAssets ? Object.fromEntries(Object.entries(job.renderAssets).filter(([k]) => k !== "blobs")) : null;
        if (job.renderAssets)
            for (const asset of job.renderAssets.blobs)
                if (!source.blobs.some(b => b.sha256 === asset.sha256))
                    source.blobs.push({ ...asset, data: Buffer.from(asset.data) });
        // A separate compressed resource keeps the executable recipe small, even for 100k+ control blocks.
        const originalMetadata: {
            id: string;
            asset: string;
            encoding: string;
            sha256: string;
            bytes: number;
        }[] = [];
        for (let at = 12; at + 8 <= size;) {
            const h = await read(at, 8), id = h.toString("ascii", 0, 4);
            let n = h.readUInt32LE(4);
            if (id === "data" && n === 0xffffffff)
                n = bwf.dataSize;
            if (["axml", "chna", "dbmd", "bext", "iXML"].includes(id)) {
                if (n > 64 * 1024 * 1024)
                    throw Error("原始 ADM 元数据附件超过 64 MiB");
                const hash = createHash("sha256"), offset = at + 8;
                const stream = Readable.from((async function* () { for (let k = 0; k < n;) {
                    const b = await read(offset + k, Math.min(65536, n - k));
                    hash.update(b);
                    yield b;
                    k += b.length;
                } })());
                const compressed: Buffer[] = [];
                const deflate = createDeflate();
                stream.on("error", error => deflate.destroy(error));
                stream.pipe(deflate);
                for await (const b of deflate)
                    compressed.push(Buffer.from(b));
                const asset = add(Buffer.concat(compressed), `original-${id}.bin.zlib`, "application/vnd.sda.adm+zlib");
                originalMetadata.push({ id, asset, encoding: "zlib", sha256: hash.digest("hex"), bytes: n });
            }
            at += 8 + n + (n & 1);
        }
        const events = adm.events.map(event => ({ ...event, position: spherical(event.pos), distanceUnit: "reference-ring" }));
        const timeline = Buffer.from(JSON.stringify({ schema: "sda.object-timeline", version: 1, sampleRate: 48000, durationSamples: frames, coordinateSystem: "ADM: X-right, Y-front, Z-up", interpolation: "forward-linear-from-event-sample", events }));
        const timelineAsset = add(deflateSync(timeline), "object-timeline.json.zlib", "application/vnd.sda.object-timeline+zlib");
        const render: any = exportRenderMetadata(job.renderSnapshot, { frames, sourceSampleRate: 48000, sourceLufs: null, programmeGainDb: 0, programmeMakeupDb: 0, volumeBalanceEnabled: false, spatialBalanceEnabled: false, peakHeadroomGainDb: 0, truePeakDbtp: null }, assets);
        render.profile = "SDAR Bed Channel With Objects";
        render.interchange.requiredFeatures.push("object-scene-v1", "sample-clock-object-automation-v1");
        render.scene.kind = "bed-objects";
        render.scene.layout = "ADM";
        render.scene.channels = adm.labels.map((label, i) => { const track = adm.tracks!.find(t => t.channel === i)!, object = adm.objectChannels.find(o => o.channel === i); const pos = track.position ?? [0, 0, 0]; if (!object && !roles[label])
            throw Error("暂不支持此 ADM 床声道：" + label); return { id: object ? `sda:object:${object.id}` : `sda:bed:${track.uid}`, kind: object ? "object" : "bed", pcmChannel: i + 1, label, name: object ? label : roles[label], displayName: track.name, sourceId: object ? `obj:${object.id}` : `bed:${i}`, objectId: object?.id, trackUid: track.uid, trackFormatId: track.trackRef, packFormatId: track.packRef, channelFormatId: track.channelFormatId, ...(!object ? { position: spherical(pos), cartesian: pos, isLfe: label === "LFE", gainDb: 0, sourceMapping: { kind: "adm-direct-speaker-role", role: label } } : {}) }; });
        render.scene.objects = adm.objectChannels.map(o => { const track = adm.tracks!.find(t => t.channel === o.channel)!; return { id: o.id, pcmChannel: o.channel + 1, sourceId: `obj:${o.id}`, name: track.name, trackUid: track.uid, channelFormatId: track.channelFormatId }; });
        render.scene.automation = { schema: "sda.object-timeline", version: 1, encoding: "zlib-json", asset: timelineAsset, eventCount: events.length, sampleRate: 48000 };
        render.output.channels = bwf.format.channels;
        render.output.channelMask = 0;
        render.baked.upmix = { applied: false, algorithm: "none" };
        render.baked.peakProtection = { policy: "source-master-preserved", headroomGainDb: 0, truePeakDbtp: null, finalOutputProtection: "renderer" };
        render.baked.dither = "source PCM24 preserved";
        render.runtimeRecipe.automationRecorded = true;
        render.provenance = { format: "ADM-BWF", originalMetadata, timingRecovery: adm.warnings ?? [], binaural: bwf.binaural };
        // Export is a scene transfer: source PCM and mixing gain are never re-rendered.
        const tags = buildMetadataChunks(source, render), extra = Buffer.concat([chunk("sdrm", Buffer.from(JSON.stringify(render))), tags.bytes]);
        const write = (b: Buffer) => { let k = 0; while (k < b.length)
            k += fs.writeSync(output!, b, k, b.length - k); };
        output = fs.openSync(job.stagingPath, "wx");
        const newHeader = Buffer.from(header);
        newHeader.write("RIFF", 0, "ascii");
        write(newHeader);
        at = 12;
        dsData = undefined;
        let dsOffset: number | undefined;
        while (at + 8 <= size) {
            check();
            const h = await read(at, 8), id = h.toString("ascii", 0, 4);
            let n = h.readUInt32LE(4);
            if (id === "ds64")
                dsData = Number((await read(at + 8, 28)).readBigUInt64LE(8));
            if (id === "data" && n === 0xffffffff) {
                if (dsData === undefined)
                    throw Error("RF64 data length missing");
                n = dsData;
            }
            const skip = ["sdrm", "id3 ", "ID3 "].includes(id) || (id === "LIST" && (await read(at + 8, 4)).toString("ascii") === "INFO");
            if (!skip) {
                if (id === "ds64")
                    dsOffset = fs.fstatSync(output).size + 8;
                for (let k = 0; k < 8 + n + (n & 1);) {
                    const b = await read(at + k, Math.min(1024 * 1024, 8 + n + (n & 1) - k));
                    write(b);
                    k += b.length;
                    progress("write", .1 + .75 * (at + k) / size);
                }
            }
            at += 8 + n + (n & 1);
        }
        write(extra);
        const finalSize = fs.fstatSync(output).size;
        if (finalSize - 8 > 0xffffffff) {
            if (dsOffset === undefined)
                throw Error("超过 RIFF 上限的母版需要 RF64 ds64");
            const b = Buffer.alloc(8);
            b.writeBigUInt64LE(BigInt(finalSize - 8));
            fs.writeSync(output, b, 0, 8, dsOffset);
            fs.writeSync(output, Buffer.from("RF64"), 0, 4, 0);
            const n = Buffer.alloc(4, 255);
            fs.writeSync(output, n, 0, 4, 4);
        }
        else {
            const b = Buffer.alloc(4);
            b.writeUInt32LE(finalSize - 8);
            fs.writeSync(output, b, 0, 4, 4);
            if (dsOffset !== undefined) {
                const b = Buffer.alloc(8);
                b.writeBigUInt64LE(BigInt(finalSize - 8));
                fs.writeSync(output, b, 0, 8, dsOffset);
            }
        }
        const after = fs.fstatSync(input), current = fs.statSync(job.inputPath);
        if ([after, current].some(s => s.size !== initial.size || s.mtimeMs !== initial.mtimeMs || s.ino !== initial.ino || s.dev !== initial.dev))
            throw Error("导出期间原始 ADM 母版发生变化，请重新导出");
        fs.fsyncSync(output);
        return { frames, channels: bwf.format.channels, bedChannels: bwf.format.channels - adm.objectChannels.length, objects: adm.objectChannels.length, objectEvents: events.length, profile: render.profile, metadata: tags.summary };
    }
    finally {
        fs.closeSync(input);
        if (output !== undefined)
            fs.closeSync(output);
    }
}
