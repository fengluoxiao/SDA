//! SDA Bed Lossless v1. Independent PCM24 blocks with predictive Rice residuals.
//! No MPEG/AAC bitstream or decoder is involved in this codec.
use crate::metadata::{self, exact, Metadata, MAX_METADATA};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::{File, OpenOptions},
    io::{Seek, Write},
    path::Path,
};
pub const CODEC: &str = "sda-bed-lossless-v1";
pub const MAGIC: &[u8; 13] = b"SDAR0002BCH2\n";
pub const BLOCK: usize = 1024;
const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut i = 0;
        while i < 8 {
            c = (c >> 1) ^ (0xedb88320u32.wrapping_mul(c & 1));
            i += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}
const CRC_TABLE: [u32; 256] = crc_table();
fn crc(b: &[u8]) -> u32 {
    let mut c = !0u32;
    for &v in b {
        c = CRC_TABLE[((c ^ v as u32) & 255) as usize] ^ (c >> 8);
    }
    !c
}
pub fn seal(mut b: Vec<u8>) -> Vec<u8> {
    b.extend(crc(&b).to_le_bytes());
    let mut out = Vec::with_capacity(b.len());
    for v in b {
        match v {
            254 => out.extend([254, 0]),
            255 => out.extend([254, 1]),
            _ => out.push(v),
        }
    }
    out
}
pub fn unseal(b: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let v = b[i];
        i += 1;
        match v {
            255 => return Err("Invalid SDAR byte escape".into()),
            254 => {
                let n = *b.get(i).ok_or("Truncated SDAR byte escape")?;
                i += 1;
                out.push(match n {
                    0 => 254,
                    1 => 255,
                    _ => return Err("Invalid SDAR byte escape".into()),
                });
            }
            _ => out.push(v),
        }
    }
    if out.len() < 4 {
        return Err("Truncated SDAR checksum".into());
    }
    let n = out.len() - 4;
    if crc(&out[..n]) != u32::from_le_bytes(out[n..].try_into().unwrap()) {
        return Err("SDAR checksum mismatch".into());
    }
    out.truncate(n);
    Ok(out)
}
fn zig(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}
fn unzig(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}
struct Bits {
    bytes: Vec<u8>,
    at: usize,
}
impl Bits {
    fn new() -> Self {
        Self {
            bytes: vec![],
            at: 0,
        }
    }
    fn bit(&mut self, v: bool) {
        if self.at % 8 == 0 {
            self.bytes.push(0)
        }
        if v {
            let n = self.bytes.len() - 1;
            self.bytes[n] |= 1 << (7 - self.at % 8)
        }
        self.at += 1;
    }
    fn rice(&mut self, v: u64, k: u8) {
        for _ in 0..(v >> k) {
            self.bit(false)
        }
        self.bit(true);
        for i in (0..k).rev() {
            self.bit(v & (1 << i) != 0)
        }
    }
}
struct ReadBits<'a> {
    b: &'a [u8],
    at: usize,
}
impl ReadBits<'_> {
    fn bit(&mut self) -> Result<bool, String> {
        let v = self.b.get(self.at / 8).ok_or("Truncated SDAR residual")?
            & (1 << (7 - self.at % 8))
            != 0;
        self.at += 1;
        Ok(v)
    }
    fn rice(&mut self, k: u8) -> Result<u64, String> {
        let mut q = 0u64;
        while !self.bit()? {
            q += 1;
            if q > 4096 {
                return Err("SDAR residual quotient too large".into());
            }
        }
        let mut v = q << k;
        for i in (0..k).rev() {
            if self.bit()? {
                v |= 1 << i
            }
        }
        Ok(v)
    }
    fn finish(&mut self) -> Result<(), String> {
        if self.b.len() * 8 - self.at > 7 {
            return Err("Extra SDAR residual data".into());
        }
        while self.at < self.b.len() * 8 {
            if self.bit()? {
                return Err("Nonzero SDAR padding".into());
            }
        }
        Ok(())
    }
}
fn predicted(v: &[i64], i: usize, order: u8) -> i64 {
    match (order, i) {
        (1, 1..) => v[i - 1],
        (2, 2..) => 2 * v[i - 1] - v[i - 2],
        (2, 1) => v[0],
        _ => 0,
    }
}
/// Causal interchannel difference plus order 0/1/2 temporal prediction.
pub fn encode_packet(pcm: &[Vec<i32>], start: u64) -> Result<Vec<u8>, String> {
    let channels = pcm.len();
    let n = pcm.first().map_or(0, Vec::len);
    if ![6, 12, 16].contains(&channels)
        || n == 0
        || n > BLOCK
        || pcm
            .iter()
            .any(|c| c.len() != n || c.iter().any(|v| !(-8388608..=8388607).contains(v)))
    {
        return Err("Invalid SDAR PCM24 block".into());
    }
    let mut h = Vec::from(b"SBL1".as_slice());
    h.extend(start.to_le_bytes());
    h.extend((n as u16).to_le_bytes());
    h.extend([channels as u8, 24]);
    let mut payload = vec![];
    for ch in 0..channels {
        let mut best = (n * 24, 0u8, 0u8, 255u8, Vec::new());
        for reference in std::iter::once(255u8).chain((0..ch).map(|c| c as u8)) {
            let values: Vec<i64> = (0..n)
                .map(|i| {
                    pcm[ch][i] as i64
                        - if reference == 255 {
                            0
                        } else {
                            pcm[reference as usize][i] as i64
                        }
                })
                .collect();
            for order in 0..=2u8 {
                let residuals: Vec<u64> = (0..n)
                    .map(|i| zig(values[i] - predicted(&values, i, order)))
                    .collect();
                let mean = residuals.iter().sum::<u64>() / n as u64;
                let k = if mean == 0 {
                    0
                } else {
                    (63 - mean.leading_zeros() as u8).min(26)
                };
                let cost = residuals
                    .iter()
                    .map(|v| (v >> k) as usize + 1 + k as usize)
                    .sum::<usize>();
                if cost < best.0 {
                    best = (cost, order + 1, k, reference, residuals)
                }
            }
        }
        let mut data = vec![];
        if best.1 == 0 {
            for &v in &pcm[ch] {
                data.extend(&v.to_le_bytes()[..3])
            }
        } else {
            let mut bits = Bits::new();
            for v in best.4 {
                bits.rice(v, best.2)
            }
            data = bits.bytes;
        }
        h.extend([best.1, best.2, best.3, 0]);
        h.extend((data.len() as u32).to_le_bytes());
        payload.extend(data);
    }
    h.extend(payload);
    Ok(seal(h))
}
pub fn decode_packet(
    b: &[u8],
    start: u64,
    channels: usize,
    frames: usize,
) -> Result<Vec<Vec<i32>>, String> {
    if b.len() > 2 * 1024 * 1024 {
        return Err("SDAR packet too large".into());
    }
    let b = unseal(b)?;
    if b.len() < 16 + channels * 8
        || &b[..4] != b"SBL1"
        || u64::from_le_bytes(b[4..12].try_into().unwrap()) != start
        || u16::from_le_bytes(b[12..14].try_into().unwrap()) as usize != frames
        || b[14] as usize != channels
        || b[15] != 24
        || frames == 0
        || frames > BLOCK
        || ![6, 12, 16].contains(&channels)
    {
        return Err("Invalid SDAR block header".into());
    }
    let mut at = 16 + channels * 8;
    let mut out: Vec<Vec<i32>> = vec![];
    for ch in 0..channels {
        let d = &b[16 + ch * 8..24 + ch * 8];
        let mode = d[0];
        let k = d[1];
        let reference = d[2];
        if mode > 3
            || k > 26
            || d[3] != 0
            || (reference != 255 && reference as usize >= ch)
            || (mode == 0 && (reference != 255 || k != 0))
        {
            return Err("Invalid SDAR predictor".into());
        }
        let len = u32::from_le_bytes(d[4..8].try_into().unwrap()) as usize;
        let end = at
            .checked_add(len)
            .filter(|&end| end <= b.len())
            .ok_or("Truncated SDAR channel")?;
        let mut values: Vec<i64> = vec![];
        if mode == 0 {
            if len != frames * 3 {
                return Err("Invalid SDAR raw length".into());
            }
            for v in b[at..end].chunks_exact(3) {
                values.push(i32::from_le_bytes([
                    v[0],
                    v[1],
                    v[2],
                    if v[2] & 128 != 0 { 255 } else { 0 },
                ]) as i64)
            }
        } else {
            let mut bits = ReadBits {
                b: &b[at..end],
                at: 0,
            };
            for i in 0..frames {
                let v = unzig(bits.rice(k)?) + predicted(&values, i, mode - 1);
                if v.abs() > 16777215 {
                    return Err("SDAR reconstructed predictor out of range".into());
                }
                values.push(v)
            }
            bits.finish()?;
        }
        let mut channel = vec![];
        for (i, v) in values.into_iter().enumerate() {
            let v = v + if reference == 255 {
                0
            } else {
                out[reference as usize][i] as i64
            };
            if !(-8388608..=8388607).contains(&v) {
                return Err("SDAR reconstructed PCM out of range".into());
            }
            channel.push(v as i32)
        }
        out.push(channel);
        at = end;
    }
    if at != b.len() {
        return Err("Extra SDAR block bytes".into());
    }
    Ok(out)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Head {
    pub schema: String,
    pub version: u32,
    pub codec: String,
    pub profile: String,
    pub sample_rate: u32,
    pub channels: usize,
    pub bits: u16,
    pub frames: u64,
    pub master_frames: u64,
    pub block_frames: usize,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub sample: u64,
    pub offset: u64,
    pub bytes: usize,
}
fn json_bytes<T: Serialize>(v: &T) -> Result<Vec<u8>, String> {
    serde_json::to_vec(v).map_err(|e| e.to_string())
}
fn write_record(file: &mut File, tag: &str, data: &[u8]) -> Result<u64, String> {
    let at = file.stream_position().map_err(|e| e.to_string())?;
    write!(file, "{tag}{:016x}\n", data.len()).map_err(|e| e.to_string())?;
    file.write_all(data).map_err(|e| e.to_string())?;
    Ok(at)
}
fn read_record(
    file: &mut File,
    at: u64,
    tag: &[u8; 4],
    limit: usize,
) -> Result<(Vec<u8>, u64), String> {
    let h = exact(file, at, 21)?;
    if &h[..4] != tag || h[20] != 10 {
        return Err("Invalid SDAR record header".into());
    }
    let n = usize::from_str_radix(
        std::str::from_utf8(&h[4..20]).map_err(|_| "Invalid SDAR record length")?,
        16,
    )
    .map_err(|_| "Invalid SDAR record length")?;
    if n > limit {
        return Err("SDAR record too large".into());
    }
    let data = exact(file, at + 21, n)?;
    Ok((data, at + 21 + n as u64))
}
pub fn wave_metadata(header: &[u8], tail: &[u8]) -> Result<Metadata, String> {
    if header.len() < 20
        || ![b"RIFF".as_slice(), b"RF64".as_slice()].contains(&&header[..4])
        || &header[8..12] != b"WAVE"
    {
        return Err("Invalid preserved WAV header".into());
    }
    let (mut render, mut id3) = (None, None);
    for (bytes, mut at) in [(header, 12usize), (tail, 0usize)] {
        while at + 8 <= bytes.len() {
            let tag = &bytes[at..at + 4];
            let n = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            if tag == b"data" {
                break;
            }
            let end = at
                .checked_add(8)
                .and_then(|a| a.checked_add(n))
                .filter(|&a| a <= bytes.len())
                .ok_or("Invalid preserved WAV metadata")?;
            match tag {
                b"sdrm" => {
                    render = Some(
                        serde_json::from_slice(&bytes[at + 8..end]).map_err(|e| e.to_string())?,
                    )
                }
                b"id3 " => id3 = Some(&bytes[at + 8..end]),
                _ => {}
            }
            at = end + (n & 1);
        }
    }
    Metadata::parse(render.ok_or("Missing SDAR render metadata")?, id3)
}
pub struct LosslessFile {
    pub file: File,
    pub head: Head,
    pub entries: Vec<Entry>,
    pub metadata: Metadata,
    pub wave_header: Vec<u8>,
    pub wave_tail: Vec<u8>,
    pub at: u64,
    queue: Vec<std::collections::VecDeque<i32>>,
}
impl LosslessFile {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        if exact(&mut file, 0, 13)? != MAGIC {
            return Err("Not SDA Bed Lossless".into());
        }
        let (raw, at) = read_record(&mut file, 13, b"HEAD", 16384)?;
        let head: Head = serde_json::from_slice(&unseal(&raw)?).map_err(|e| e.to_string())?;
        let (raw, at) = read_record(&mut file, at, b"WAVH", MAX_METADATA * 2)?;
        let header = unseal(&raw)?;
        let (raw, audio_start) = read_record(&mut file, at, b"WAVT", MAX_METADATA * 2)?;
        let tail = unseal(&raw)?;
        let size = file.metadata().map_err(|e| e.to_string())?.len();
        if size < 25 {
            return Err("Truncated SDAR index".into());
        }
        let foot = exact(&mut file, size - 25, 25)?;
        if &foot[..8] != b"SDARIDX2" || foot[24] != 10 {
            return Err("Missing SDAR index footer".into());
        }
        let index_at = u64::from_str_radix(
            std::str::from_utf8(&foot[8..24]).map_err(|_| "Invalid SDAR index offset")?,
            16,
        )
        .map_err(|_| "Invalid SDAR index offset")?;
        let (raw, end) = read_record(&mut file, index_at, b"INDX", MAX_METADATA * 2)?;
        if end != size - 25 {
            return Err("Invalid SDAR index boundary".into());
        }
        let entries: Vec<Entry> =
            serde_json::from_slice(&unseal(&raw)?).map_err(|e| e.to_string())?;
        let metadata = wave_metadata(&header, &tail)?;
        let mut source = Self::new(file, head, entries, metadata)?;
        let mut next = audio_start;
        for e in &source.entries {
            if e.offset != next + 21 {
                return Err("Invalid SDAR block offset".into());
            }
            let h = exact(&mut source.file, next, 21)?;
            let len = usize::from_str_radix(
                std::str::from_utf8(&h[4..20]).map_err(|_| "Invalid SDAR block length")?,
                16,
            )
            .map_err(|_| "Invalid SDAR block length")?;
            if &h[..4] != b"AUD2" || h[20] != 10 || len != e.bytes {
                return Err("Invalid SDAR audio record".into());
            }
            next = e.offset + e.bytes as u64;
        }
        if next != index_at {
            return Err("Invalid SDAR audio boundary".into());
        }
        source.wave_header = header;
        source.wave_tail = tail;
        Ok(source)
    }
    pub fn new(
        file: File,
        head: Head,
        entries: Vec<Entry>,
        metadata: Metadata,
    ) -> Result<Self, String> {
        if head.schema != "sda.bed-file"
            || head.version != 2
            || head.codec != CODEC
            || head.profile != "SDAR Bed Channel"
            || head.bits != 24
            || head.block_frames != BLOCK
            || ![44100, 48000, 96000].contains(&head.sample_rate)
            || head.channels != metadata.channels()
            || head.master_frames != metadata.master_frames()
            || head.frames
                != (head.master_frames as f64 * head.sample_rate as f64 / 48000.).round() as u64
            || head.frames == 0
            || entries.len() > 1_000_000
            || entries.len() as u64 != head.frames.div_ceil(BLOCK as u64)
        {
            return Err("Invalid SDA lossless configuration".into());
        }
        let size = file.metadata().map_err(|e| e.to_string())?.len();
        let mut end = 0;
        for (i, e) in entries.iter().enumerate() {
            if e.sample != i as u64 * BLOCK as u64
                || e.bytes == 0
                || e.bytes > 2 * 1024 * 1024
                || e.offset < end
                || e.offset
                    .checked_add(e.bytes as u64)
                    .is_none_or(|v| v > size)
            {
                return Err("Invalid SDA lossless packet index".into());
            }
            end = e.offset + e.bytes as u64;
        }
        let queue = (0..head.channels)
            .map(|_| std::collections::VecDeque::new())
            .collect();
        Ok(Self {
            file,
            head,
            entries,
            metadata,
            wave_header: vec![],
            wave_tail: vec![],
            at: 0,
            queue,
        })
    }
    pub fn seek_base(&mut self, target: u64) -> Result<u64, String> {
        let rate = self.head.sample_rate as u64;
        let gcd = |mut a: u64, mut b: u64| {
            while b != 0 {
                (a, b) = (b, a % b)
            }
            a
        };
        let period = rate / gcd(rate, 48000);
        let desired = (target as u128 * rate as u128 / 48000) as u64;
        let base = if rate == 48000 {
            desired
        } else {
            desired.saturating_sub(4096) / period * period
        };
        self.at = base.min(self.head.frames);
        for q in &mut self.queue {
            q.clear()
        }
        Ok((self.at as u128 * 48000 / rate as u128) as u64)
    }
    pub fn read_int(&mut self, max: usize) -> Result<Vec<Vec<i32>>, String> {
        let n = (self.head.frames - self.at).min(max.clamp(1, 4096) as u64) as usize;
        let mut out = vec![Vec::with_capacity(n); self.head.channels];
        while out[0].len() < n {
            if self.queue[0].is_empty() {
                let index = (self.at / BLOCK as u64) as usize;
                let e = &self.entries[index];
                let count = (self.head.frames - e.sample).min(BLOCK as u64) as usize;
                let packet = exact(&mut self.file, e.offset, e.bytes)?;
                let pcm = decode_packet(&packet, e.sample, self.head.channels, count)?;
                let skip = (self.at - e.sample) as usize;
                for (ch, data) in pcm.iter().enumerate() {
                    self.queue[ch].extend(data[skip..].iter().copied())
                }
            }
            let count = (n - out[0].len()).min(self.queue[0].len());
            for (ch, data) in out.iter_mut().enumerate() {
                data.extend(self.queue[ch].drain(..count))
            }
            self.at += count as u64;
        }
        Ok(out)
    }
    pub fn read(&mut self, max: usize) -> Result<Vec<Vec<f32>>, String> {
        Ok(self
            .read_int(max)?
            .into_iter()
            .map(|c| c.into_iter().map(|v| v as f32 / 8388608.).collect())
            .collect())
    }
}

pub fn encode(input: &Path, output: &Path, rate: u32) -> Result<Head, String> {
    use rubato::{
        Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
    };
    if ![44100, 48000, 96000].contains(&rate) {
        return Err("Unsupported SDA sample rate".into());
    }
    let (mut source, format, metadata) = metadata::read_master(input, true)?;
    let size = source.metadata().map_err(|e| e.to_string())?.len();
    let mut header = metadata::metadata_bytes(&mut source, 0, format.offset)?;
    if &header[..4] == b"SDAR" {
        header[..4].copy_from_slice(if size - 8 > u32::MAX as u64 {
            b"RF64"
        } else {
            b"RIFF"
        });
        header[8..12].copy_from_slice(b"WAVE");
    }
    let tail = metadata::metadata_bytes(
        &mut source,
        format.offset + format.bytes,
        size - format.offset - format.bytes,
    )?;
    let head = Head {
        schema: "sda.bed-file".into(),
        version: 2,
        codec: CODEC.into(),
        profile: "SDAR Bed Channel".into(),
        sample_rate: rate,
        channels: format.channels,
        bits: 24,
        frames: (metadata.master_frames() as f64 * rate as f64 / 48000.).round() as u64,
        master_frames: metadata.master_frames(),
        block_frames: BLOCK,
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| e.to_string())?;
    file.write_all(MAGIC).map_err(|e| e.to_string())?;
    write_record(&mut file, "HEAD", &seal(json_bytes(&head)?))?;
    write_record(&mut file, "WAVH", &seal(header))?;
    write_record(&mut file, "WAVT", &seal(tail))?;
    let mut resampler = if rate != 48000 {
        Some(
            SincFixedIn::<f32>::new(
                rate as f64 / 48000.,
                1.,
                SincInterpolationParameters {
                    sinc_len: 128,
                    f_cutoff: 0.95,
                    oversampling_factor: 128,
                    interpolation: SincInterpolationType::Cubic,
                    window: WindowFunction::BlackmanHarris2,
                },
                1024,
                format.channels,
            )
            .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    let mut queues: Vec<std::collections::VecDeque<i32>> = (0..format.channels)
        .map(|_| std::collections::VecDeque::new())
        .collect();
    let (mut read_at, mut written) = (0u64, 0u64);
    let mut entries = vec![];
    while written < head.frames {
        let n = (head.frames - written).min(BLOCK as u64) as usize;
        while queues[0].len() < n {
            let count = (head.master_frames - read_at).min(BLOCK as u64) as usize;
            if count == 0 && resampler.is_none() {
                return Err("SDA input ended early".into());
            }
            let bytes = exact(
                &mut source,
                format.offset + read_at * format.channels as u64 * 3,
                count * format.channels * 3,
            )?;
            let mut pcm = vec![vec![0i32; count]; format.channels];
            for (i, frame) in bytes.chunks_exact(format.channels * 3).enumerate() {
                for (ch, v) in frame.chunks_exact(3).enumerate() {
                    pcm[ch][i] = i32::from_le_bytes([
                        v[0],
                        v[1],
                        v[2],
                        if v[2] & 128 != 0 { 255 } else { 0 },
                    ]);
                }
            }
            read_at += count as u64;
            if let Some(src) = &mut resampler {
                let floats: Vec<Vec<f32>> = pcm
                    .into_iter()
                    .map(|c| {
                        let mut c: Vec<f32> = c.into_iter().map(|v| v as f32 / 8388608.).collect();
                        c.resize(BLOCK, 0.);
                        c
                    })
                    .collect();
                let out = src.process(&floats, None).map_err(|e| e.to_string())?;
                for (ch, data) in out.iter().enumerate() {
                    let values=data.iter().map(|&v|{let value=(v as f64*8388608.).round();if !value.is_finite()||!(-8388608. ..=8388607.).contains(&value){return Err("Sample-rate conversion exceeds PCM24 range; use the 48 kHz master".to_string())}Ok(value as i32)}).collect::<Result<Vec<_>,String>>()?;
                    queues[ch].extend(values)
                }
            } else {
                for (ch, data) in pcm.into_iter().enumerate() {
                    queues[ch].extend(data)
                }
            }
        }
        let pcm: Vec<Vec<i32>> = queues.iter_mut().map(|q| q.drain(..n).collect()).collect();
        let packet = encode_packet(&pcm, written)?;
        let at = write_record(&mut file, "AUD2", &packet)?;
        entries.push(Entry {
            sample: written,
            offset: at + 21,
            bytes: packet.len(),
        });
        written += n as u64;
        if entries.len() % 32 == 0 {
            println!(
                "{}",
                json!({"type":"progress","progress":written as f64/head.frames as f64})
            );
        }
    }
    let index = write_record(&mut file, "INDX", &seal(json_bytes(&entries)?))?;
    write!(file, "SDARIDX2{index:016x}\n").map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    Ok(head)
}
pub fn restore(input: &Path, output: &Path) -> Result<(), String> {
    let mut source = LosslessFile::open(input)?;
    if source.head.sample_rate != 48000 || source.head.frames != source.head.master_frames {
        return Err("Rate-converted delivery is not an original WAV master".into());
    }
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| e.to_string())?;
    out.write_all(&source.wave_header)
        .map_err(|e| e.to_string())?;
    while source.at < source.head.frames {
        let pcm = source.read_int(4096)?;
        let mut bytes = Vec::with_capacity(pcm[0].len() * pcm.len() * 3);
        for i in 0..pcm[0].len() {
            for ch in &pcm {
                bytes.extend(&ch[i].to_le_bytes()[..3])
            }
        }
        out.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    out.write_all(&source.wave_tail)
        .map_err(|e| e.to_string())?;
    out.sync_all().map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packets() {
        for channels in [6, 12, 16] {
            for n in [1, 31, 1024] {
                let mut seed = 5u32;
                let pcm: Vec<Vec<i32>> = (0..channels)
                    .map(|ch| {
                        (0..n)
                            .map(|i| {
                                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                                match ch % 5 {
                                    0 => 0,
                                    1 => {
                                        if i % 2 == 0 {
                                            -8388608
                                        } else {
                                            8388607
                                        }
                                    }
                                    2 => (seed as i32) >> 8,
                                    3 => (i as i32 * 1000) - 500000,
                                    _ => i as i32 * 1000,
                                }
                            })
                            .collect()
                    })
                    .collect();
                let packet = encode_packet(&pcm, 512).unwrap();
                assert!(!packet.contains(&255));
                assert_eq!(decode_packet(&packet, 512, channels, n).unwrap(), pcm);
                let mut bad = packet.clone();
                bad[10] ^= 8;
                assert!(decode_packet(&bad, 512, channels, n).is_err());
                assert!(decode_packet(&packet, 0, channels, n).is_err());
            }
        }
    }
    #[test]
    fn escapes() {
        for b in [vec![254], vec![254, 2], vec![255], vec![]] {
            assert!(unseal(&b).is_err())
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(unseal(&seal(all.clone())).unwrap(), all);
    }
}
