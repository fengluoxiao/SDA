use crate::{
    aac::AacDecoder,
    metadata::{exact, metadata_bytes, Metadata},
};
use serde_json::Value;
use std::{collections::VecDeque, fs::File, path::Path};
const MAX_SAMPLES: usize = 1_000_000;
#[derive(Clone, Debug)]
struct Atom {
    id: [u8; 4],
    body: u64,
    end: u64,
}
fn be32(b: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_be_bytes(
        b.get(at..at + 4)
            .ok_or("Truncated SDAR MP4 field")?
            .try_into()
            .unwrap(),
    ))
}
fn be64(b: &[u8], at: usize) -> Result<u64, String> {
    Ok(u64::from_be_bytes(
        b.get(at..at + 8)
            .ok_or("Truncated SDAR MP4 field")?
            .try_into()
            .unwrap(),
    ))
}
fn atoms(file: &mut File, start: u64, end: u64) -> Result<Vec<Atom>, String> {
    let mut at = start;
    let mut out = vec![];
    while at + 8 <= end {
        let h = exact(file, at, 8)?;
        let mut size = be32(&h, 0)? as u64;
        let id = h[4..8].try_into().unwrap();
        let mut head = 8;
        if size == 1 {
            size = be64(&exact(file, at + 8, 8)?, 0)?;
            head = 16;
        } else if size == 0 {
            size = end - at;
        }
        if size < head || at.checked_add(size).is_none_or(|n| n > end) {
            return Err("Invalid SDAR MP4 atom size".into());
        }
        out.push(Atom {
            id,
            body: at + head,
            end: at + size,
        });
        if out.len() > 100_000 {
            return Err("Too many SDAR MP4 atoms".into());
        }
        at += size;
    }
    if at != end {
        return Err("Truncated SDAR MP4 atom header".into());
    }
    Ok(out)
}
fn child(file: &mut File, parent: &Atom, id: &[u8; 4]) -> Result<Atom, String> {
    atoms(file, parent.body, parent.end)?
        .into_iter()
        .find(|a| &a.id == id)
        .ok_or_else(|| format!("Missing SDAR MP4 {}", String::from_utf8_lossy(id)))
}
fn optional(file: &mut File, parent: &Atom, id: &[u8; 4]) -> Result<Option<Atom>, String> {
    Ok(atoms(file, parent.body, parent.end)?
        .into_iter()
        .find(|a| &a.id == id))
}
fn bytes(file: &mut File, atom: &Atom) -> Result<Vec<u8>, String> {
    metadata_bytes(file, atom.body, atom.end - atom.body)
}
fn child_bytes(file: &mut File, parent: &Atom, id: &[u8; 4]) -> Result<Vec<u8>, String> {
    let atom = child(file, parent, id)?;
    bytes(file, &atom)
}
fn count(b: &[u8], at: usize) -> Result<usize, String> {
    let n = be32(b, at)? as usize;
    if n > MAX_SAMPLES {
        return Err("SDAR MP4 sample table exceeds supported size".into());
    }
    Ok(n)
}
fn descriptor(b: &[u8], at: &mut usize) -> Result<(u8, std::ops::Range<usize>), String> {
    let id = *b.get(*at).ok_or("Missing AAC descriptor")?;
    *at += 1;
    let mut n = 0usize;
    let mut ended = false;
    for _ in 0..4 {
        let next = *b.get(*at).ok_or("Truncated AAC descriptor")?;
        *at += 1;
        n = (n << 7) | (next as usize & 127);
        if next & 128 == 0 {
            ended = true;
            break;
        }
    }
    if !ended || at.checked_add(n).is_none_or(|end| end > b.len()) {
        return Err("Invalid AAC descriptor size".into());
    }
    let result = *at..*at + n;
    *at += n;
    Ok((id, result))
}
fn asc_from_esds(b: &[u8]) -> Result<Vec<u8>, String> {
    let mut at = 4;
    let (id, es) = descriptor(b, &mut at)?;
    if id != 3 || es.len() < 3 {
        return Err("Invalid AAC ES descriptor".into());
    }
    let flags = b[es.start + 2];
    let mut at = es.start + 3;
    if flags & 128 != 0 {
        at += 2;
    }
    if flags & 64 != 0 {
        let n = *b.get(at).ok_or("Invalid AAC URL descriptor")? as usize;
        at += 1 + n;
    }
    if flags & 32 != 0 {
        at += 2;
    }
    let (id, decoder) = descriptor(b, &mut at)?;
    if id != 4 || decoder.len() < 13 || b[decoder.start] != 0x40 {
        return Err("SDAR delivery requires AAC".into());
    }
    let mut at = decoder.start + 13;
    while at < decoder.end {
        let (id, data) = descriptor(b, &mut at)?;
        if id == 5 {
            if data.len() < 2 || data.len() > 64 {
                return Err("Invalid AAC configuration size".into());
            }
            return Ok(b[data].to_vec());
        }
    }
    Err("AAC configuration missing".into())
}
struct Track {
    decoder: AacDecoder,
    offsets: Vec<u64>,
    sizes: Vec<u32>,
    next: usize,
    trim: u64,
    initial_trim: u64,
    map: Vec<usize>,
    queue: Vec<VecDeque<f32>>,
}
impl Track {
    fn decode_next(&mut self, file: &mut File) -> Result<bool, String> {
        if self.next >= self.sizes.len() {
            return Ok(false);
        }
        let index = self.next;
        self.next += 1;
        let packet = exact(file, self.offsets[index], self.sizes[index] as usize)?;
        let frame = self.decoder.decode(&packet)?;
        let frames = frame.channels[0].len();
        if frames == 0 {
            // FAAD suppresses its first full 1024-sample LC frame. MP4 stts
            // durations can be shorter at a rate-converted boundary; they are not
            // the number of PCM samples discarded by the decoder.
            self.trim = self.trim.saturating_sub(1024);
            return Ok(true);
        }
        let drop = self.trim.min(frames as u64) as usize;
        self.trim -= drop as u64;
        let order = match self.map.len() {
            2 => [2, 3]
                .iter()
                .map(|role| {
                    frame
                        .positions
                        .iter()
                        .position(|p| p == role)
                        .ok_or("AAC stereo channel positions missing")
                })
                .collect::<Result<Vec<_>, _>>()?,
            6 => [2, 3, 1, 9, 6, 7]
                .iter()
                .map(|role| {
                    frame
                        .positions
                        .iter()
                        .position(|p| p == role)
                        .or_else(|| match role {
                            6 => frame.positions.iter().position(|p| *p == 4),
                            7 => frame.positions.iter().position(|p| *p == 5),
                            _ => None,
                        })
                        .ok_or("AAC 5.1 channel positions missing")
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err("Unsupported SDA AAC group".into()),
        };
        for (slot, ch) in order.iter().enumerate() {
            self.queue[slot].extend(frame.channels[*ch][drop..].iter().copied());
        }
        Ok(true)
    }
}
pub struct AacSource {
    file: File,
    tracks: Vec<Track>,
    pub metadata: Metadata,
    pub transport: Value,
    pub sample_rate: u32,
    pub frames: u64,
    at: u64,
}
impl AacSource {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let length = file.metadata().map_err(|e| e.to_string())?.len();
        let top = atoms(&mut file, 0, length)?;
        let ftyp = top.iter().find(|a| a.id == *b"ftyp").ok_or("Not SDA M4A")?;
        if exact(&mut file, ftyp.body, 4)? != b"sda1" {
            return Err("Not SDA private M4A delivery".into());
        }
        let value = |file: &mut File, id: &[u8; 4]| -> Result<Value, String> {
            let atom = top
                .iter()
                .find(|a| &a.id == id)
                .ok_or("Missing SDA M4A metadata")?;
            serde_json::from_slice(&bytes(file, atom)?).map_err(|e| e.to_string())
        };
        let transport = value(&mut file, b"sdaf")?;
        let render = value(&mut file, b"sdrm")?;
        let id3 = top
            .iter()
            .find(|a| a.id == *b"sdid")
            .map(|atom| bytes(&mut file, atom))
            .transpose()?;
        let metadata = Metadata::parse(render, id3.as_deref())?;
        if transport["schema"] != "sda.delivery"
            || transport["version"] != 1
            || transport["codec"] != "aac-lc"
            || transport["mobileDefaultLayout"] != "7.1.4"
        {
            return Err("Unsupported SDA M4A transport version".into());
        }
        let sample_rate = transport["sampleRate"]
            .as_u64()
            .filter(|r| [44100, 48000, 96000].contains(r))
            .ok_or("Unsupported SDA M4A rate")? as u32;
        let frames = transport["frames"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or("Invalid SDA M4A length")?;
        if transport["masterFrames"].as_u64() != Some(metadata.master_frames())
            || frames.abs_diff(
                (metadata.master_frames() as f64 * sample_rate as f64 / 48000.).round() as u64,
            ) > 1
        {
            return Err("SDA M4A duration metadata mismatch".into());
        }
        let moov = top
            .iter()
            .find(|a| a.id == *b"moov")
            .ok_or("SDA M4A moov missing")?;
        let mdats = top.iter().filter(|a| a.id == *b"mdat").collect::<Vec<_>>();
        let mut tables = std::collections::BTreeMap::new();
        for trak in atoms(&mut file, moov.body, moov.end)?
            .into_iter()
            .filter(|a| a.id == *b"trak")
        {
            let mdia = child(&mut file, &trak, b"mdia")?;
            let handler = child(&mut file, &mdia, b"hdlr")?;
            if exact(&mut file, handler.body + 8, 4)? != b"soun" {
                continue;
            }
            let tkhd = child_bytes(&mut file, &trak, b"tkhd")?;
            let id = be32(&tkhd, if tkhd[0] == 1 { 20 } else { 12 })?;
            if tables.insert(id, (trak, mdia)).is_some() {
                return Err("Duplicate SDA M4A track ID".into());
            }
        }
        let groups = transport["tracks"]
            .as_array()
            .ok_or("SDA M4A channel groups missing")?;
        if groups.len() != 4 || metadata.channels() != 12 || tables.len() != 4 {
            return Err("Unsupported SDA M4A bed grouping".into());
        }
        let mut tracks = Vec::new();
        let mut assigned = std::collections::BTreeSet::new();
        for group in groups {
            let id = group["trackId"]
                .as_u64()
                .filter(|n| *n <= u32::MAX as u64)
                .ok_or("Invalid SDA M4A track ID")? as u32;
            let (trak, mdia) = tables.remove(&id).ok_or("Missing SDA M4A bed group")?;
            let map = group["channels"]
                .as_array()
                .ok_or("Missing SDA M4A channel mapping")?
                .iter()
                .map(|n| {
                    n.as_u64()
                        .filter(|n| *n < 12)
                        .map(|n| n as usize)
                        .ok_or("Invalid SDA M4A channel mapping")
                })
                .collect::<Result<Vec<_>, _>>()?;
            if ![2, 6].contains(&map.len()) || map.iter().any(|n| !assigned.insert(*n)) {
                return Err("Duplicate SDA M4A channel mapping".into());
            }
            let mdhd = child_bytes(&mut file, &mdia, b"mdhd")?;
            let timescale = be32(&mdhd, if mdhd[0] == 1 { 20 } else { 12 })?;
            if timescale == 0 {
                return Err("SDA M4A timescale is zero".into());
            }
            let minf = child(&mut file, &mdia, b"minf")?;
            let stbl = child(&mut file, &minf, b"stbl")?;
            let stsd = child(&mut file, &stbl, b"stsd")?;
            let entry_header = exact(&mut file, stsd.body, 16)?;
            if be32(&entry_header, 4)? != 1 || &entry_header[12..16] != b"sdaA" {
                return Err("SDA M4A private AAC identity missing".into());
            }
            let entry_size = be32(&entry_header, 8)? as u64;
            let entry = Atom {
                id: *b"sdaA",
                body: stsd.body + 8 + 36,
                end: stsd.body + 8 + entry_size,
            };
            if entry.end > stsd.end || entry.body > entry.end {
                return Err("Invalid SDA AAC entry".into());
            }
            let config = asc_from_esds(&child_bytes(&mut file, &entry, b"sdac")?)?;
            let decoder = AacDecoder::new(&config)?;
            if decoder.sample_rate != sample_rate || decoder.channels != map.len() {
                return Err("SDA AAC channel/rate declaration mismatch".into());
            }
            let stsz = child_bytes(&mut file, &stbl, b"stsz")?;
            let default = be32(&stsz, 4)?;
            let sample_count = count(&stsz, 8)?;
            let mut sizes = Vec::with_capacity(sample_count);
            for i in 0..sample_count {
                let size = if default != 0 {
                    default
                } else {
                    be32(&stsz, 12 + i * 4)?
                };
                if size == 0 || size > 1024 * 1024 {
                    return Err("Invalid SDA AAC packet length".into());
                }
                sizes.push(size);
            }
            let stsc = child_bytes(&mut file, &stbl, b"stsc")?;
            let mappings = (0..count(&stsc, 4)?)
                .map(|i| {
                    Ok((
                        be32(&stsc, 8 + i * 12)?,
                        be32(&stsc, 12 + i * 12)?,
                        be32(&stsc, 16 + i * 12)?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            if mappings.first().map(|v| v.0) != Some(1)
                || mappings.windows(2).any(|w| w[1].0 <= w[0].0)
                || mappings.iter().any(|v| v.1 == 0 || v.2 != 1)
            {
                return Err("Invalid SDA MP4 sample-to-chunk map".into());
            }
            let wide = optional(&mut file, &stbl, b"co64")?;
            let chunk_atom = if let Some(a) = wide.clone() {
                a
            } else {
                child(&mut file, &stbl, b"stco")?
            };
            let chunks = bytes(&mut file, &chunk_atom)?;
            let mut offsets = Vec::with_capacity(sample_count);
            let mut run = 0;
            for c in 0..count(&chunks, 4)? {
                let number = c as u32 + 1;
                while run + 1 < mappings.len() && mappings[run + 1].0 <= number {
                    run += 1;
                }
                let mut offset = if wide.is_some() {
                    be64(&chunks, 8 + c * 8)?
                } else {
                    be32(&chunks, 8 + c * 4)? as u64
                };
                for _ in 0..mappings[run].1 {
                    let i = offsets.len();
                    if i >= sample_count {
                        return Err("Extra SDA AAC samples".into());
                    }
                    let size = sizes[i] as u64;
                    if !mdats.iter().any(|m| {
                        offset >= m.body && offset.checked_add(size).is_some_and(|end| end <= m.end)
                    }) {
                        return Err("SDA AAC packet is outside media data".into());
                    }
                    offsets.push(offset);
                    offset += size;
                }
            }
            if offsets.len() != sample_count {
                return Err("Missing SDA AAC samples".into());
            }
            let stts = child_bytes(&mut file, &stbl, b"stts")?;
            let mut durations = Vec::with_capacity(sample_count);
            for i in 0..count(&stts, 4)? {
                let n = count(&stts, 8 + i * 8)?;
                let duration = be32(&stts, 12 + i * 8)?;
                if durations.len() + n > sample_count || duration == 0 {
                    return Err("Invalid SDA AAC time table".into());
                }
                durations.extend(std::iter::repeat_n(duration, n));
            }
            if durations.len() != sample_count {
                return Err("Missing SDA AAC timing".into());
            }
            let mut trim = 0u64;
            if let Some(edts) = optional(&mut file, &trak, b"edts")? {
                let elst = child_bytes(&mut file, &edts, b"elst")?;
                let entries = count(&elst, 4)?;
                if entries > 2 {
                    return Err("Unsupported SDA AAC edit list".into());
                }
                for i in 0..entries {
                    let at = 8 + i * if elst[0] == 1 { 20 } else { 12 };
                    let media = if elst[0] == 1 {
                        be64(&elst, at + 8)? as i64
                    } else {
                        be32(&elst, at + 4)? as i32 as i64
                    };
                    if media >= 0 {
                        trim = ((media as u128 * sample_rate as u128) / timescale as u128) as u64;
                        break;
                    }
                }
            }
            if trim > 4096 {
                return Err("Unsupported SDA AAC priming edit".into());
            }
            if std::env::var_os("SDA_FORMAT_DEBUG").is_some() {
                eprintln!("SDAR AAC track={id} rate={sample_rate} timescale={timescale} trim={trim} first_duration={}",durations[0]);
            }
            tracks.push(Track {
                decoder,
                offsets,
                sizes,
                next: 0,
                trim,
                initial_trim: trim,
                map: map.clone(),
                queue: (0..map.len()).map(|_| VecDeque::new()).collect(),
            });
        }
        if assigned.len() != 12 {
            return Err("SDA M4A bed extension is incomplete".into());
        }
        Ok(Self {
            file,
            tracks,
            metadata,
            transport,
            sample_rate,
            frames,
            at: 0,
        })
    }
    pub fn seek_base(&mut self, target_master: u64) -> Result<u64, String> {
        fn gcd(mut a: u64, mut b: u64) -> u64 {
            while b != 0 {
                let n = a % b;
                a = b;
                b = n;
            }
            a
        }
        let target = (u128::from(target_master) * u128::from(self.sample_rate) / 48000) as u64;
        let period = u64::from(self.sample_rate) / gcd(u64::from(self.sample_rate), 48000);
        let packet_period = period / gcd(period, 1024);
        let packet = (target / 1024).saturating_sub(1);
        let packet = packet - packet % packet_period;
        for track in &mut self.tracks {
            if track.initial_trim != 1024 {
                return Err("Unsupported SDA AAC seek priming".into());
            }
            if packet as usize >= track.sizes.len() {
                return Err("SDA AAC seek outside sample table".into());
            }
            track.decoder.restart()?;
            track.next = packet as usize;
            track.trim = if packet == 0 { track.initial_trim } else { 0 };
            for q in &mut track.queue {
                q.clear();
            }
        }
        self.at = packet * 1024;
        Ok((u128::from(self.at) * 48000 / u128::from(self.sample_rate)) as u64)
    }
    pub fn read(&mut self, max_frames: usize) -> Result<Vec<Vec<f32>>, String> {
        let count = ((self.frames - self.at) as usize).min(max_frames.clamp(1, 4096));
        if count == 0 {
            return Ok(vec![vec![]; 12]);
        }
        let mut out = vec![vec![0.; count]; 12];
        for track in &mut self.tracks {
            while track.queue[0].len() < count {
                if !track.decode_next(&mut self.file)? {
                    return Err("SDA AAC ended before its declared timeline".into());
                }
            }
            for (local, global) in track.map.iter().enumerate() {
                for sample in &mut out[*global] {
                    *sample = track.queue[local].pop_front().unwrap();
                }
            }
        }
        self.at += count as u64;
        Ok(out)
    }
}
