package app.sda.mobile.sda

import java.io.File
import java.io.IOException
import java.io.RandomAccessFile

/** Metadata-only preflight. Never copies audio, rewrites the source or guesses missing samples. */
internal object Mp4AudioBounds {
    data class Report(val samples: Int, val referencedBytes: Long)
    private data class Box(val kind: String, val begin: Int, val end: Int)
    private data class Span(val begin: Long, val end: Long)
    private fun invalid(reason: String): Nothing = throw IOException("MP4 音频索引无效：$reason")
    private fun requireIndex(ok: Boolean, reason: String) { if (!ok) invalid(reason) }
    private fun u32(data: ByteArray, at: Int): Long {
        requireIndex(at >= 0 && at <= data.size - 4, "元数据已截断")
        var value = 0L
        for (i in at until at + 4) value = (value shl 8) or (data[i].toLong() and 255)
        return value
    }
    private fun u64(data: ByteArray, at: Int, boxSize: Boolean = false): Long {
        val hi = u32(data, at); val lo = u32(data, at + 4)
        if (hi > Int.MAX_VALUE) {
            if (boxSize) return Long.MAX_VALUE // oversized mdat is bounded by the physical file below
            invalid("64 位 chunk 偏移超出文件范围")
        }
        return (hi shl 32) or lo
    }
    private fun kind(data: ByteArray, at: Int) = String(data, at, 4, Charsets.US_ASCII)
    private fun boxes(data: ByteArray, begin: Int = 0, end: Int = data.size): List<Box> {
        val result = ArrayList<Box>(); var at = begin
        while (at < end) {
            requireIndex(end - at >= 8 && result.size < 100_000, "box 边界")
            var size = u32(data, at); var header = 8
            if (size == 1L) { requireIndex(end - at >= 16, "64 位 box 头"); size = u64(data, at + 8, true); header = 16 }
            if (size == 0L) size = (end - at).toLong()
            requireIndex(size >= header && size <= end - at, "box 长度")
            result.add(Box(kind(data, at + 4), at + header, at + size.toInt())); at += size.toInt()
        }
        return result
    }
    private fun child(data: ByteArray, parent: Box, name: String) = boxes(data, parent.begin, parent.end).firstOrNull { it.kind == name }
    private fun read(file: RandomAccessFile, at: Long, count: Int): ByteArray {
        requireIndex(count in 0..64 * 1024 * 1024, "读取长度")
        val result = ByteArray(count); file.seek(at); file.readFully(result); return result
    }
    fun validateEac3(source: File): Report? = RandomAccessFile(source, "r").use { file ->
        val length = file.length(); var at = 0L; var moov: ByteArray? = null
        val mdats = ArrayList<Span>(); var trailingProblem = false
        while (at < length) {
            val remaining = length - at
            if (remaining < 8) { trailingProblem = true; break }
            val data = read(file, at, minOf(16L, remaining).toInt())
            val name = kind(data, 4); var size = u32(data, 0); var header = 8L
            if (size == 1L) {
                if (data.size < 16) { if (name == "moov") invalid("文件头"); trailingProblem = true; break }
                size = u64(data, 8, true); header = 16
            }
            if (size == 0L) size = remaining
            if (size < header || size > remaining) {
                if (name == "moov") invalid("文件 box 范围")
                if (name == "mdat" && size >= header) mdats.add(Span(at + header, length))
                trailingProblem = true; break
            }
            when (name) {
                "moov" -> {
                    requireIndex(moov == null && size - header <= 64 * 1024 * 1024, "moov 长度或重复")
                    moov = read(file, at + header, (size - header).toInt())
                }
                "mdat" -> mdats.add(Span(at + header, at + size))
            }
            at += size
        }
        val metadata = moov
        if (metadata == null) { if (trailingProblem) invalid("文件 box 范围"); return@use null }
        val root = Box("moov", 0, metadata.size)
        if (child(metadata, root, "mvex") != null) return@use null // fragmented files retain the platform reader
        val candidates = ArrayList<Box>()
        for (track in boxes(metadata).filter { it.kind == "trak" }) {
            val mdia = child(metadata, track, "mdia") ?: continue
            val handler = child(metadata, mdia, "hdlr") ?: continue
            if (handler.end - handler.begin < 12 || kind(metadata, handler.begin + 8) != "soun") continue
            val minf = child(metadata, mdia, "minf") ?: continue
            val stbl = child(metadata, minf, "stbl") ?: continue
            val stsd = child(metadata, stbl, "stsd") ?: continue
            if (stsd.end - stsd.begin < 8 || u32(metadata, stsd.begin + 4) != 1L) continue
            val entries = boxes(metadata, stsd.begin + 8, stsd.end)
            if (entries.size != 1 || entries[0].kind != "ec-3") continue
            val entry = entries[0]
            if (entry.end - entry.begin < 28 || child(metadata, Box("entry", entry.begin + 28, entry.end), "sinf") != null) continue
            val dinf = child(metadata, minf, "dinf") ?: return@use null
            val dref = child(metadata, dinf, "dref") ?: return@use null
            if (dref.end - dref.begin < 8 || u32(metadata, dref.begin + 4) != 1L) return@use null
            val references = boxes(metadata, dref.begin + 8, dref.end)
            if (references.size != 1 || references[0].kind != "url " || references[0].end - references[0].begin < 4 || (u32(metadata, references[0].begin) and 1L) != 1L) return@use null
            candidates.add(stbl)
        }
        if (candidates.size != 1) return@use null // do not bind a different track on multi-track containers
        val table = candidates[0]
        val sz = child(metadata, table, "stsz") ?: return@use null
        val sc = child(metadata, table, "stsc") ?: return@use null
        val co32 = child(metadata, table, "stco")
        val co = co32 ?: child(metadata, table, "co64") ?: invalid("缺少 chunk 偏移")
        val width = if (co32 != null) 4 else 8
        requireIndex(sz.end - sz.begin >= 12 && sc.end - sc.begin >= 8 && co.end - co.begin >= 8, "样本表长度")
        val fixed = u32(metadata, sz.begin + 4); val count = u32(metadata, sz.begin + 8)
        requireIndex(count in 1L..4_194_304L, "样本数量")
        requireIndex(fixed != 0L || sz.end - sz.begin >= 12 + count * 4, "stsz 样本表已截断")
        val sizes = IntArray(count.toInt()) { i ->
            val size = if (fixed == 0L) u32(metadata, sz.begin + 12 + i * 4) else fixed
            requireIndex(size <= 1024 * 1024, "单包过大"); size.toInt()
        }
        val chunks = u32(metadata, co.begin + 4); val runs = u32(metadata, sc.begin + 4)
        requireIndex(chunks in 1L..4_194_304L && co.end - co.begin >= 8 + chunks * width, "chunk 数量或长度")
        requireIndex(runs in 1L..chunks && sc.end - sc.begin >= 8 + runs * 12, "stsc 数量或长度")
        var previous = 0L
        for (i in 0 until runs.toInt()) {
            val pos = sc.begin + 8 + i * 12; val first = u32(metadata, pos)
            requireIndex(first in 1L..chunks && first > previous && (i != 0 || first == 1L) && u32(metadata, pos + 4) > 0, "stsc 范围")
            if (u32(metadata, pos + 8) != 1L) return@use null
            previous = first
        }
        var run = 0; var sample = 0; var missing = 0L; var bytes = 0L
        for (chunk in 0 until chunks.toInt()) {
            while (run + 1 < runs && u32(metadata, sc.begin + 8 + (run + 1) * 12) <= chunk + 1) run++
            var offset = if (width == 4) u32(metadata, co.begin + 8 + chunk * width) else u64(metadata, co.begin + 8 + chunk * width)
            val samples = minOf(u32(metadata, sc.begin + 12 + run * 12), count - sample).toInt()
            repeat(samples) {
                val size = sizes[sample].toLong()
                requireIndex(offset <= Long.MAX_VALUE - size, "样本偏移溢出")
                val stop = offset + size
                if (mdats.none { offset >= it.begin && stop <= it.end }) {
                    if (stop > length) missing = maxOf(missing, stop - length) else invalid("样本超出 mdat")
                }
                bytes += size; offset = stop; sample++
            }
            if (sample == count.toInt()) break
        }
        requireIndex(sample == count.toInt(), "chunk 少于样本数量")
        if (missing > 0) throw IOException("音频文件不完整：末尾缺少 $missing 字节，请重新下载或重新传输原文件。")
        Report(sample, bytes)
    }
}
