//! Bounded native MPEG-H and Audio Vivid extraction. Only compressed access units are copied;
//! no PCM, analysis results, or persistent content cache is created here.
use serde::Serialize;
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
};
const MAX_INDEX: usize = 32 * 1024 * 1024;
const MAX_SAMPLES: usize = 4_194_304;
const MAX_PACKET: usize = 1024 * 1024;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedTrack {
    pub codec: String,
    pub duration_ms: f64,
    pub packet_count: usize,
    pub bytes: u64,
}
#[derive(Clone, Copy)]
struct Atom<'a> {
    kind: [u8; 4],
    body: &'a [u8],
}
fn invalid() -> String {
    "Invalid MP4 audio sample index".into()
}
fn uint(b: &[u8], at: usize, width: usize) -> Result<u64, String> {
    let end = at.checked_add(width).ok_or_else(invalid)?;
    let bytes = b.get(at..end).ok_or_else(invalid)?;
    Ok(bytes.iter().fold(0, |n, &x| (n << 8) | u64::from(x)))
}
fn atoms(mut b: &[u8]) -> Result<Vec<Atom<'_>>, String> {
    let mut result = Vec::new();
    while !b.is_empty() {
        let n = uint(b, 0, 4)?;
        let kind = b.get(4..8).ok_or_else(invalid)?.try_into().unwrap();
        let (n, head) = if n == 1 {
            (uint(b, 8, 8)?, 16)
        } else if n == 0 {
            (b.len() as u64, 8)
        } else {
            (n, 8)
        };
        let n = usize::try_from(n).map_err(|_| invalid())?;
        if n < head || n > b.len() {
            return Err(invalid());
        }
        result.push(Atom {
            kind,
            body: &b[head..n],
        });
        b = &b[n..];
    }
    Ok(result)
}
fn child<'a>(b: &'a [u8], kind: &[u8; 4]) -> Result<Option<&'a [u8]>, String> {
    Ok(atoms(b)?
        .into_iter()
        .find(|a| &a.kind == kind)
        .map(|a| a.body))
}
fn required<'a>(b: &'a [u8], kind: &[u8; 4]) -> Result<&'a [u8], String> {
    child(b, kind)?.ok_or_else(invalid)
}
struct Index {
    codec: String,
    config: Vec<u8>,
    sizes: Vec<usize>,
    chunks: Vec<u64>,
    runs: Vec<(usize, usize)>,
    duration_ms: f64,
}
fn table(b: &[u8], start: usize, stride: usize) -> Result<usize, String> {
    let count = uint(b, 4, 4)? as usize;
    if count > MAX_SAMPLES
        || start
            .checked_add(count.checked_mul(stride).ok_or_else(invalid)?)
            .ok_or_else(invalid)?
            > b.len()
    {
        return Err(invalid());
    }
    Ok(count)
}
fn parse_index(moov: &[u8]) -> Result<Option<Index>, String> {
    if child(moov, b"mvex")?.is_some() {
        return Ok(None);
    } // fragmented MP4 stays with the established demuxer
    let tracks = atoms(moov)?;
    // Match Mp4Demuxer's selection: MPEG-H tracks first, then AV3A,
    // retaining file order within each group even in mixed-codec containers.
    for audio_vivid in [false, true] {
        for track in tracks.iter().filter(|a| a.kind == *b"trak") {
            let Some(mdia) = child(track.body, b"mdia")? else {
                continue;
            };
            if required(mdia, b"hdlr")?.get(8..12) != Some(b"soun") {
                continue;
            }
            let stbl = required(required(mdia, b"minf")?, b"stbl")?;
            let stsd = required(stbl, b"stsd")?;
            let entries = atoms(stsd.get(8..).ok_or_else(invalid)?)?;
            let Some(entry) = entries.first() else {
                continue;
            };
            if if audio_vivid {
                entry.kind != *b"av3a"
            } else {
                entry.kind != *b"mha1" && entry.kind != *b"mhm1"
            } {
                continue;
            }
            if entries.len() != 1 || uint(stsd, 4, 4)? != 1 {
                return Ok(None);
            }
            if audio_vivid
                && (!(1..=16).contains(&uint(entry.body, 16, 2)?) || uint(entry.body, 24, 4)? == 0)
            {
                return Err("Invalid Audio Vivid audio sample entry".into());
            }
            // The standard version-0 audio sample entry is 28 bytes before children.
            // Alternate descriptions/versions or external data references use fallback.
            if uint(entry.body, 8, 2)? != 0 || uint(entry.body, 6, 2)? != 1 {
                return Ok(None);
            }
            let extension = entry.body.get(28..).ok_or_else(invalid)?;
            if child(extension, b"sinf")?.is_some() {
                return Err("Encrypted audio track cannot be extracted".into());
            }
            let minf = required(mdia, b"minf")?;
            let dref = required(required(minf, b"dinf")?, b"dref")?;
            let refs = atoms(dref.get(8..).ok_or_else(invalid)?)?;
            if uint(dref, 4, 4)? != 1
                || refs.len() != 1
                || refs[0].kind != *b"url "
                || uint(refs[0].body, 0, 4)? != 1
            {
                return Ok(None);
            }
            let mut config = Vec::new();
            if entry.kind == *b"mha1" {
                let c = required(extension, b"mhaC")?;
                let n = uint(c, 3, 2)? as usize;
                if n == 0 {
                    return Err("MPEG-H mha1 requires mhaC configuration".into());
                }
                config.extend_from_slice(c.get(5..5 + n).ok_or_else(invalid)?);
            }
            if let Some(ctts) = child(stbl, b"ctts")? {
                let n = table(ctts, 8, 8)?;
                if uint(ctts, 0, 1)? > 1 {
                    return Ok(None);
                }
                for i in 0..n {
                    if uint(ctts, 12 + i * 8, 4)? != 0 {
                        return Ok(None);
                    }
                }
            }
            let mdhd = required(mdia, b"mdhd")?;
            let version = uint(mdhd, 0, 1)?;
            let (rate, duration) = match version {
                0 => (uint(mdhd, 12, 4)?, uint(mdhd, 16, 4)?),
                1 => (uint(mdhd, 20, 4)?, uint(mdhd, 24, 8)?),
                _ => return Ok(None),
            };
            if rate == 0 {
                return Err(invalid());
            }
            // An edit list does not alter extracted access-unit bytes. Keep the same
            // mdhd duration and AU sequence as Mp4Demuxer (no trimming/re-encoding).
            let Some(sz) = child(stbl, b"stsz")? else {
                return Ok(None);
            };
            let fixed = uint(sz, 4, 4)? as usize;
            let total = uint(sz, 8, 4)? as usize;
            if total == 0 || total > MAX_SAMPLES || (fixed == 0 && sz.len() < 12 + total * 4) {
                return Err(invalid());
            }
            let mut sizes = Vec::with_capacity(total);
            for i in 0..total {
                let n = if fixed == 0 {
                    uint(sz, 12 + i * 4, 4)? as usize
                } else {
                    fixed
                };
                if n == 0 || n > MAX_PACKET {
                    return Err(invalid());
                }
                sizes.push(n);
            }
            let (offsets, width) = if let Some(co) = child(stbl, b"stco")? {
                (co, 4)
            } else if let Some(co) = child(stbl, b"co64")? {
                (co, 8)
            } else {
                return Err(invalid());
            };
            let n = table(offsets, 8, width)?;
            if n == 0 {
                return Err(invalid());
            }
            let chunks = (0..n)
                .map(|i| uint(offsets, 8 + i * width, width))
                .collect::<Result<Vec<_>, _>>()?;
            let sc = required(stbl, b"stsc")?;
            let n = table(sc, 8, 12)?;
            if n == 0 || n > chunks.len() {
                return Err(invalid());
            }
            let mut runs = Vec::with_capacity(n);
            for i in 0..n {
                let first = uint(sc, 8 + i * 12, 4)? as usize;
                let count = uint(sc, 12 + i * 12, 4)? as usize;
                if uint(sc, 16 + i * 12, 4)? != 1 {
                    return Ok(None);
                }
                if first == 0
                    || first > chunks.len()
                    || count == 0
                    || count > total
                    || runs.last().map_or(first != 1, |&(old, _)| first <= old)
                {
                    return Err(invalid());
                }
                runs.push((first, count));
            }
            let ts = required(stbl, b"stts")?;
            let n = table(ts, 8, 8)?;
            let mut frames = 0u64;
            for i in 0..n {
                let count = uint(ts, 8 + i * 8, 4)?;
                let duration = uint(ts, 12 + i * 8, 4)?;
                if duration == 0 {
                    return Err(invalid());
                }
                frames = frames.checked_add(count).ok_or_else(invalid)?;
            }
            if frames != total as u64 {
                return Err(invalid());
            }
            return Ok(Some(Index {
                codec: String::from_utf8(entry.kind.to_vec()).unwrap(),
                config,
                sizes,
                chunks,
                runs,
                duration_ms: duration as f64 / rate as f64 * 1000.,
            }));
        }
    }
    Ok(None)
}
/// Header emitted identically to packages/core/src/mhas.ts, label=1.
fn mhas_header(kind: u8, len: usize) -> Vec<u8> {
    let mut bits = Vec::new();
    fn put(bits: &mut Vec<bool>, v: u64, n: u32) {
        for i in (0..n).rev() {
            bits.push((v >> i) & 1 != 0);
        }
    }
    put(&mut bits, kind.into(), 3);
    put(&mut bits, 1, 2);
    if len < 2047 {
        put(&mut bits, len as u64, 11);
    } else {
        put(&mut bits, 2047, 11);
        let rest = len - 2047;
        if rest < 0xffffff {
            put(&mut bits, rest as u64, 24);
        } else {
            put(&mut bits, 0xffffff, 24);
            put(&mut bits, (rest - 0xffffff) as u64, 24);
        }
    }
    let mut bytes = vec![0; bits.len() / 8];
    for (i, bit) in bits.into_iter().enumerate() {
        if bit {
            bytes[i / 8] |= 1 << (7 - i % 8);
        }
    }
    bytes
}
/// Returns None before opening `output` for unsupported container variants.
fn open_index(source: &Path) -> Result<Option<(BufReader<File>, Index)>, String> {
    let mut input =
        BufReader::with_capacity(1024 * 1024, File::open(source).map_err(|e| e.to_string())?);
    let length = input.get_ref().metadata().map_err(|e| e.to_string())?.len();
    let mut moov = None;
    let mut mdats = Vec::new();
    let mut offset = 0u64;
    let mut top_count = 0usize;
    while offset < length {
        top_count += 1;
        if top_count > 1_000_000 {
            return Err(invalid());
        }
        input
            .seek(SeekFrom::Start(offset))
            .map_err(|e| e.to_string())?;
        let mut h = [0u8; 8];
        input.read_exact(&mut h).map_err(|e| e.to_string())?;
        let n = u32::from_be_bytes(h[..4].try_into().unwrap()) as u64;
        let (n, head) = if n == 1 {
            input.read_exact(&mut h[..8]).map_err(|e| e.to_string())?;
            (u64::from_be_bytes(h), 16)
        } else if n == 0 {
            (length - offset, 8)
        } else {
            (n, 8)
        };
        // Header kind must survive reading an extended length.
        input
            .seek(SeekFrom::Start(offset + 4))
            .map_err(|e| e.to_string())?;
        let mut kind = [0; 4];
        input.read_exact(&mut kind).map_err(|e| e.to_string())?;
        let stop = offset.checked_add(n).ok_or_else(invalid)?;
        if n < head || stop > length {
            return Err("MP4 file is truncated or has an invalid box length".into());
        }
        if kind == *b"moof" {
            return Ok(None);
        }
        if kind == *b"moov" {
            if moov.is_some() || n - head > MAX_INDEX as u64 {
                return Ok(None);
            }
            input
                .seek(SeekFrom::Start(offset + head))
                .map_err(|e| e.to_string())?;
            let mut bytes = vec![0; (n - head) as usize];
            input.read_exact(&mut bytes).map_err(|e| e.to_string())?;
            moov = Some(bytes);
        } else if kind == *b"mdat" {
            mdats.push(offset + head..stop);
        }
        offset = stop;
    }
    let Some(moov) = moov else { return Ok(None) };
    let Some(index) = parse_index(&moov)? else {
        return Ok(None);
    };
    // Validate the complete map before touching the output file.
    let mut sample: usize = 0;
    let mut run = 0;
    for (chunk, &start) in index.chunks.iter().enumerate() {
        while run + 1 < index.runs.len() && index.runs[run + 1].0 <= chunk + 1 {
            run += 1;
        }
        let count = index.runs[run].1;
        let end_sample = sample.checked_add(count).ok_or_else(invalid)?;
        if end_sample > index.sizes.len() {
            return Err(invalid());
        }
        let bytes = index.sizes[sample..end_sample]
            .iter()
            .try_fold(0u64, |n, &size| {
                n.checked_add(size as u64).ok_or_else(invalid)
            })?;
        let end = start.checked_add(bytes).ok_or_else(invalid)?;
        if !mdats
            .iter()
            .any(|range| start >= range.start && end <= range.end)
        {
            return Err("Audio sample extends outside MP4 data (file may be incomplete)".into());
        }
        sample = end_sample;
    }
    if sample != index.sizes.len() {
        return Err(invalid());
    }
    Ok(Some((input,index)))
}

/// Caller owns and retires the session working file, just as for JS extraction.
pub fn extract(
    source: impl AsRef<Path>,
    output: impl AsRef<Path>,
) -> Result<Option<ExtractedTrack>, String> {
    let source = source.as_ref();
    let output = output.as_ref();
    if source == output {
        return Err("Extraction output must differ from input".into());
    }
    let Some((mut input,index)) = open_index(source)? else { return Ok(None); };
    // create_new prevents accidentally replacing the input or another user's file.
    let mut writer = BufWriter::with_capacity(
        1024 * 1024,
        File::options()
            .write(true)
            .create_new(true)
            .open(output)
            .map_err(|e| e.to_string())?,
    );
    let result = (|| {
        let raw = index.codec == "mha1";
        let mut written = 0u64;
        if raw {
            let head = mhas_header(1, index.config.len());
            writer
                .write_all(&head)
                .and_then(|_| writer.write_all(&index.config))
                .map_err(|e| e.to_string())?;
            written += (head.len() + index.config.len()) as u64;
        }
        let mut packet = vec![0; MAX_PACKET];
        let mut sample: usize = 0;
        let mut run = 0;
        for (chunk, &start) in index.chunks.iter().enumerate() {
            while run + 1 < index.runs.len() && index.runs[run + 1].0 <= chunk + 1 {
                run += 1;
            }
            // BufReader reuses buffered sequential bytes within/between chunks.
            if input.stream_position().map_err(|e| e.to_string())? != start {
                input
                    .seek(SeekFrom::Start(start))
                    .map_err(|e| e.to_string())?;
            }
            for _ in 0..index.runs[run].1 {
                let n = index.sizes[sample];
                input
                    .read_exact(&mut packet[..n])
                    .map_err(|e| e.to_string())?;
                if raw {
                    let header = mhas_header(2, n);
                    writer.write_all(&header).map_err(|e| e.to_string())?;
                    written += header.len() as u64;
                }
                writer.write_all(&packet[..n]).map_err(|e| e.to_string())?;
                written += n as u64;
                sample += 1;
            }
        }
        writer.flush().map_err(|e| e.to_string())?;
        Ok(Some(ExtractedTrack {
            codec: index.codec,
            duration_ms: index.duration_ms,
            packet_count: sample,
            bytes: written,
        }))
    })();
    drop(writer);
    if result.is_err() {
        let _ = std::fs::remove_file(output);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn atom(kind: &[u8; 4], body: Vec<u8>) -> Vec<u8> {
        let mut b = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        b.extend(kind);
        b.extend(body);
        b
    }
    fn words(v: &[u32]) -> Vec<u8> {
        v.iter().flat_map(|n| n.to_be_bytes()).collect()
    }
    fn fixture(raw: bool, co64: bool, tail: bool, fragmented: bool) -> Vec<u8> {
        fixture_codec(if raw { b"mha1" } else { b"mhm1" }, co64, tail, fragmented)
    }
    fn fixture_codec(codec: &[u8; 4], co64: bool, tail: bool, fragmented: bool) -> Vec<u8> {
        fn movie(codec: &[u8; 4], co64: bool, offset: u64, fragmented: bool) -> Vec<u8> {
            let mut entry = vec![0; 28];
            entry[7] = 1;
            entry[16..18].copy_from_slice(&2u16.to_be_bytes());
            entry[24..28].copy_from_slice(&(48000u32 << 16).to_be_bytes());
            if codec == b"mha1" {
                entry.extend(atom(b"mhaC", vec![1, 0, 0, 0, 2, 0xab, 0xcd]));
            }
            let mut sd = words(&[0, 1]);
            sd.extend(atom(codec, entry));
            let mut stbl = atom(b"stsd", sd);
            stbl.extend(atom(b"stsz", words(&[0, 0, 3, 3, 2, 4])));
            stbl.extend(atom(b"stsc", words(&[0, 1, 1, 3, 1])));
            stbl.extend(atom(b"stts", words(&[0, 1, 3, 1024])));
            let mut co = words(&[0, 1]);
            if co64 {
                co.extend(offset.to_be_bytes());
            } else {
                co.extend((offset as u32).to_be_bytes());
            }
            stbl.extend(atom(if co64 { b"co64" } else { b"stco" }, co));
            let mut dref = words(&[0, 1]);
            dref.extend(atom(b"url ", words(&[1])));
            let mut minf = atom(b"dinf", atom(b"dref", dref));
            minf.extend(atom(b"stbl", stbl));
            let mut hdlr = vec![0; 12];
            hdlr[8..12].copy_from_slice(b"soun");
            let mut mdia = atom(b"hdlr", hdlr);
            mdia.extend(atom(b"mdhd", words(&[0, 0, 0, 48000, 3072])));
            mdia.extend(atom(b"minf", minf));
            let mut moov = atom(b"trak", atom(b"mdia", mdia));
            if fragmented {
                moov.extend(atom(b"mvex", vec![]));
            }
            atom(b"moov", moov)
        }
        let ftyp = atom(b"ftyp", b"isom0000".to_vec());
        let provisional = movie(codec, co64, 0, fragmented);
        let offset = (ftyp.len() + 8 + if tail { 0 } else { provisional.len() }) as u64;
        let moov = movie(codec, co64, offset, fragmented);
        let mdat = atom(b"mdat", vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
        let mut b = ftyp;
        if tail {
            b.extend(mdat);
            b.extend(moov);
        } else {
            b.extend(moov);
            b.extend(mdat);
        }
        b
    }
    fn run(bytes: &[u8]) -> (Result<Option<ExtractedTrack>, String>, Vec<u8>) {
        let dir = std::env::temp_dir().join(format!(
            "sda-mp4-extract-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        let input = dir.join("input.mp4");
        let output = dir.join("output.mhas");
        std::fs::write(&input, bytes).unwrap();
        let result = extract(&input, &output);
        let data = std::fs::read(&output).unwrap_or_default();
        std::fs::remove_dir_all(dir).unwrap();
        (result, data)
    }
    #[test]
    fn byte_exact_mhm1_front_tail_stco_co64() {
        for tail in [false, true] {
            for co64 in [false, true] {
                let (r, b) = run(&fixture(false, co64, tail, false));
                let r = r.unwrap().unwrap();
                assert_eq!(r.codec, "mhm1");
                assert_eq!(r.duration_ms, 64.);
                assert_eq!(r.packet_count, 3);
                assert_eq!(b, (1..=9).collect::<Vec<_>>());
            }
        }
    }
    #[test]
    fn av3a_is_exact_raw_access_units_for_front_tail_and_co64() {
        for tail in [false, true] {
            for co64 in [false, true] {
                let (result, bytes) = run(&fixture_codec(b"av3a", co64, tail, false));
                let result = result.unwrap().unwrap();
                assert_eq!(result.codec, "av3a");
                assert_eq!(result.packet_count, 3);
                assert_eq!(result.duration_ms, 64.);
                assert_eq!(result.bytes, 9);
                assert_eq!(bytes, (1..=9).collect::<Vec<_>>());
            }
        }
    }
    #[test]
    fn av3a_invalid_sample_entry_is_rejected_like_shared_demuxer() {
        for (offset, width) in [(20, 2), (28, 4)] {
            let mut file = fixture_codec(b"av3a", false, true, false);
            let at = file.windows(4).position(|w| w == b"av3a").unwrap();
            file[at + offset..at + offset + width].fill(0);
            assert!(run(&file).0.is_err());
        }
    }
    #[test]
    fn av3a_fragmented_stays_on_existing_demuxer_without_output() {
        let (result, bytes) = run(&fixture_codec(b"av3a", true, true, true));
        assert!(result.unwrap().is_none());
        assert!(bytes.is_empty());
    }
    #[test]
    fn mixed_track_selection_keeps_existing_mpegh_priority() {
        let vivid = fixture_codec(b"av3a", false, true, false);
        let mpegh = fixture(false, false, true, false);
        let va = atoms(&vivid).unwrap();
        let ma = atoms(&mpegh).unwrap();
        let mut output = atom(
            b"ftyp",
            va.iter()
                .find(|a| a.kind == *b"ftyp")
                .unwrap()
                .body
                .to_vec(),
        );
        output.extend(atom(
            b"mdat",
            va.iter()
                .find(|a| a.kind == *b"mdat")
                .unwrap()
                .body
                .to_vec(),
        ));
        let mut movie = va
            .iter()
            .find(|a| a.kind == *b"moov")
            .unwrap()
            .body
            .to_vec();
        movie.extend_from_slice(ma.iter().find(|a| a.kind == *b"moov").unwrap().body);
        output.extend(atom(b"moov", movie));
        let (result, bytes) = run(&output);
        assert_eq!(result.unwrap().unwrap().codec, "mhm1");
        assert_eq!(bytes, (1..=9).collect::<Vec<_>>());
    }
    #[test]
    fn mha1_emits_configuration_and_each_au_header() {
        let (r, b) = run(&fixture(true, true, true, false));
        assert!(r.unwrap().is_some());
        assert_eq!(
            b,
            vec![0x28, 2, 0xab, 0xcd, 0x48, 3, 1, 2, 3, 0x48, 2, 4, 5, 0x48, 4, 6, 7, 8, 9]
        );
    }
    #[test]
    fn fragmented_falls_back_without_output() {
        let (r, b) = run(&fixture(false, false, false, true));
        assert!(r.unwrap().is_none());
        assert!(b.is_empty());
    }
    #[test]
    fn truncated_file_rejected() {
        let mut b = fixture(false, false, true, false);
        b.pop();
        let (r, b) = run(&b);
        assert!(r.is_err());
        assert!(b.is_empty());
    }
    #[test]
    fn samples_outside_mdat_rejected() {
        let mut b = fixture(false, false, false, false);
        let at = b.windows(4).position(|w| w == b"stco").unwrap();
        b[at + 12..at + 16].copy_from_slice(&1u32.to_be_bytes());
        assert!(run(&b).0.is_err());
    }
    #[test]
    fn invalid_chunk_run_rejected() {
        let mut b = fixture(false, false, false, false);
        let at = b.windows(4).position(|w| w == b"stsc").unwrap();
        b[at + 12..at + 16].copy_from_slice(&0u32.to_be_bytes());
        assert!(run(&b).0.is_err());
    }
    #[test]
    fn sample_description_switch_falls_back() {
        let mut b = fixture(false, false, false, false);
        let at = b.windows(4).position(|w| w == b"stsc").unwrap();
        b[at + 20..at + 24].copy_from_slice(&2u32.to_be_bytes());
        assert!(run(&b).0.unwrap().is_none());
    }
    #[test]
    fn external_data_reference_falls_back() {
        let mut b = fixture(false, false, false, false);
        let at = b.windows(4).position(|w| w == b"url ").unwrap();
        b[at + 4..at + 8].copy_from_slice(&0u32.to_be_bytes());
        assert!(run(&b).0.unwrap().is_none());
    }
    #[test]
    fn missing_configuration_rejected() {
        let mut b = fixture(true, false, false, false);
        let at = b.windows(4).position(|w| w == b"mhaC").unwrap();
        b[at + 7..at + 9].copy_from_slice(&0u16.to_be_bytes());
        assert!(run(&b).0.is_err());
    }
    #[test]
    fn escaped_packet_length_matches_known_headers() {
        assert_eq!(mhas_header(2, 2046), [0x4f, 0xfe]);
        assert_eq!(mhas_header(2, 2047), [0x4f, 0xff, 0, 0, 0]);
        assert_eq!(mhas_header(2, 2048), [0x4f, 0xff, 0, 0, 1]);
    }
    #[test]
    fn virtual_compressed_views_match_extraction_at_random_offsets() {
        for codec in [b"mha1",b"mhm1",b"av3a"] { for co64 in [false,true] {for tail in [false,true] {
            let dir=std::env::temp_dir().join(format!("sda-view-{}-{}-{co64}-{tail}",std::process::id(),String::from_utf8_lossy(codec)));
            std::fs::create_dir_all(&dir).unwrap();let input=dir.join("source.mp4");let output=dir.join("track");
            std::fs::write(&input,fixture_codec(codec,co64,tail,false)).unwrap();extract(&input,&output).unwrap().unwrap();
            let expected=std::fs::read(&output).unwrap();
            let mut view=if codec==b"av3a"{MhasFile::open_av3a(&input)}else{MhasFile::open(&input)}.unwrap().unwrap();
            let mut all=vec![];view.read_to_end(&mut all).unwrap();assert_eq!(all,expected);
            for start in 0..=expected.len(){view.seek(SeekFrom::Start(start as u64)).unwrap();let mut got=vec![0;3];let n=view.read(&mut got).unwrap();assert_eq!(&got[..n],&expected[start..(start+3).min(expected.len())]);}
            assert!(view.seek(SeekFrom::End(-1)).is_ok());let mut byte=[0];view.read_exact(&mut byte).unwrap();assert_eq!(byte[0],*expected.last().unwrap());
            assert!(view.seek(SeekFrom::Start(100000)).is_ok());assert_eq!(view.read(&mut byte).unwrap(),0);
            std::fs::remove_dir_all(dir).unwrap();
        }}}
    }
    #[test]
    fn existing_output_is_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("sda-existing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("source");
        let out = dir.join("output");
        std::fs::write(&input, fixture(false, false, false, false)).unwrap();
        std::fs::write(&out, b"keep").unwrap();
        assert!(extract(&input, &out).is_err());
        assert_eq!(std::fs::read(&out).unwrap(), b"keep");
        assert!(extract(&input, &input).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// An indexed view over the original MP4 bytes. No extracted track or PCM file.
/// It presents exactly the MHAS bytes emitted by `extract`, including mha1 headers.
pub struct MhasFile { input: BufReader<File>, spans: Vec<(u64,Span)>, length:u64, position:u64 }
enum Span { Memory(Vec<u8>), Source{offset:u64,length:usize} }
impl MhasFile {
 pub fn open(path:impl AsRef<Path>)->Result<Option<Self>,String>{
  Self::open_kind(path.as_ref(),false)
 }
 pub fn open_av3a(path:impl AsRef<Path>)->Result<Option<Self>,String>{Self::open_kind(path.as_ref(),true)}
 fn open_kind(path:&Path,av3a:bool)->Result<Option<Self>,String>{
  let Some((input,index))=open_index(path)?else{return Ok(None)};
  if if av3a {index.codec!="av3a"} else {!matches!(index.codec.as_str(),"mha1"|"mhm1")} {return Ok(None);}
  let raw=index.codec=="mha1";let mut view=Self{input,spans:Vec::new(),length:0,position:0};
  if raw {let mut config=mhas_header(1,index.config.len());config.extend_from_slice(&index.config);view.push(Span::Memory(config))?;}
  let(mut sample,mut run)=(0,0);
  for(chunk,&offset)in index.chunks.iter().enumerate(){
   while run+1<index.runs.len()&&index.runs[run+1].0<=chunk+1{run+=1;}
   let mut at=offset;
   for _ in 0..index.runs[run].1{let bytes=index.sizes[sample];if raw{view.push(Span::Memory(mhas_header(2,bytes)))?;}
    view.push(Span::Source{offset:at,length:bytes})?;at+=bytes as u64;sample+=1;
   }
  }
  Ok(Some(view))
 }
 fn push(&mut self,span:Span)->Result<(),String>{let n=match &span{Span::Memory(b)=>b.len(),Span::Source{length,..}=>*length};self.spans.push((self.length,span));self.length=self.length.checked_add(n as u64).ok_or("virtual MHAS length overflow")?;Ok(())}
}
impl Read for MhasFile {
 fn read(&mut self,out:&mut[u8])->std::io::Result<usize>{
  let mut written=0;
  while written<out.len()&&self.position<self.length{
   let index=self.spans.partition_point(|(at,_)|*at<=self.position).saturating_sub(1);let(start,span)=&self.spans[index];let relative=(self.position-start)as usize;
   let total=match span{Span::Memory(bytes)=>bytes.len(),Span::Source{length,..}=>*length};let count=(total-relative).min(out.len()-written);
   match span{Span::Memory(bytes)=>out[written..written+count].copy_from_slice(&bytes[relative..relative+count]),Span::Source{offset,..}=>{self.input.seek(SeekFrom::Start(offset+relative as u64))?;self.input.read_exact(&mut out[written..written+count])?;}}
   self.position+=count as u64;written+=count;
  }Ok(written)
 }
}
impl Seek for MhasFile {
 fn seek(&mut self,from:SeekFrom)->std::io::Result<u64>{let position=match from{SeekFrom::Start(n)=>i128::from(n),SeekFrom::Current(n)=>i128::from(self.position)+i128::from(n),SeekFrom::End(n)=>i128::from(self.length)+i128::from(n)};self.position=u64::try_from(position).map_err(|_|std::io::Error::new(std::io::ErrorKind::InvalidInput,"invalid virtual MHAS seek"))?;Ok(self.position)}
}
