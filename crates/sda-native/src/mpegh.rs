//! Host adapter for the *same* pinned Ittiam C decoder and capture bridge used by
//! Windows's MPEG-H WASM. No MPEG-H audio decoding is implemented in Rust.
use std::{collections::VecDeque, slice, sync::atomic::{AtomicBool, Ordering}};
use sda_core::{FrameData, ObjectEvent, ObjectChannelDecl};

extern "C" {
    fn sda_open_layout(raw: i32, layout: i32) -> i32;
    fn sda_close();
    fn sda_input() -> *mut u8;
    fn sda_capacity() -> i32;
    fn sda_decode(n: i32) -> i32;
    fn sda_info(key: i32) -> i32;
    fn sda_pcm() -> *const f32;
    fn sda_metadata() -> *const f32;
    fn sda_rendered() -> *const u8;
    fn sda_bed(ch: i32) -> i32;
}
// bridge.c is a singleton (one Windows worker). Native ownership must be exclusive
// even when separate MobileEngine instances are opened on different host threads.
static OWNED: AtomicBool = AtomicBool::new(false);
const MAX_PENDING: usize = 1024 * 1024;

pub struct MpeghDecoder {
    pending: Vec<u8>,
    frames: VecDeque<(FrameData, Vec<Vec<f32>>)>,
    sample_pos: u64,
    speaker_output: bool,
}
impl MpeghDecoder {
    pub fn new() -> Result<Self, String> { Self::with_layout(2) }
    /// Upstream MPEG-H object/HOA renderer, CICP 19 (7.1.4). No KU100 processing.
    pub fn new_7_1_4() -> Result<Self, String> { Self::with_layout(19) }
    fn with_layout(layout: i32) -> Result<Self, String> {
        if OWNED.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return Err("MPEG-H decoder already has an active owner".into());
        }
        let error = unsafe { sda_open_layout(0, layout) };
        if error != 0 {
            unsafe { sda_close(); }
            OWNED.store(false, Ordering::Release);
            return Err(format!("MPEG-H create: 0x{:x}", error as u32));
        }
        Ok(Self { pending: Vec::new(), frames: VecDeque::new(), sample_pos: 0, speaker_output: layout == 19 })
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.pending.extend_from_slice(bytes);
        let mut cursor = 0;
        while cursor < self.pending.len() {
            let Some(end) = access_unit(&self.pending, cursor)? else { break };
            let n = end - cursor;
            let capacity = unsafe { sda_capacity() };
            if capacity <= 0 || n > capacity as usize { return Err("MPEG-H access unit exceeds decoder capacity".into()); }
            unsafe { std::ptr::copy_nonoverlapping(self.pending[cursor..end].as_ptr(), sda_input(), n); }
            let error = unsafe { sda_decode(n as i32) };
            let used = unsafe { sda_info(0) };
            if error == 0x1000 || error == 0x1800 {
                if n == capacity as usize { return Err("MPEG-H access unit exceeds input capacity".into()); }
                break;
            }
            if error != 0 { return Err(format!("MPEG-H decode: 0x{:x}", error as u32)); }
            if used < 0 || used as usize > n { return Err("MPEG-H invalid consumption".into()); }
            if let Some(frame) = self.take_frame()? {
                // bridge output is overwritten by the next access unit. Capture
                // the matching reference here, not after an entire input chunk.
                let reference = if self.speaker_output { Vec::new() } else { self.reference_stereo()? };
                self.frames.push_back((frame, reference));
            }
            if used == 0 { break; }
            cursor += used as usize;
        }
        self.pending.drain(..cursor);
        if self.pending.len() > MAX_PENDING { return Err("MPEG-H input stalled".into()); }
        Ok(())
    }
    pub fn next_frame(&mut self) -> Option<FrameData> { self.frames.pop_front().map(|(frame, _)| frame) }
    pub fn next_frame_with_reference(&mut self) -> Option<(FrameData, Vec<Vec<f32>>)> { self.frames.pop_front() }
    pub fn flush(&self) -> Result<(), String> {
        if self.pending.is_empty() { Ok(()) } else { Err(format!("MPEG-H truncated stream ({} bytes remain)", self.pending.len())) }
    }
    fn take_frame(&mut self) -> Result<Option<FrameData>, String> {
        if self.speaker_output { return self.take_speakers(); }
        // All pointers remain owned by this exclusive bridge instance, and are copied
        // before the next decode call. Bounds mirror bridge.c / MpeghDecoder.ts.
        let info = |key| unsafe { sda_info(key) };
        let n = info(3);
        if n == 0 { return Ok(None); }
        if !(1..=4096).contains(&n) { return Err("MPEG-H invalid captured sample count".into()); }
        let n = n as usize;
        let (objects, beds, hoa, rate) = (info(5), info(6), info(7), info(2));
        if rate <= 0 || objects < 0 || beds < 0 || objects + beds > 64 { return Err("MPEG-H invalid source format".into()); }
        let mut labels = Vec::<String>::new();
        let mut events = Vec::new();
        let mut declarations = Vec::new();
        let channels;
        if objects > 0 {
            if hoa != 0 { return Err("MPEG-H mixed objects/HOA is not supported yet".into()); }
            if info(4) != beds + objects { return Err("MPEG-H source mapping mismatch".into()); }
            let mut bed_targets = Vec::new();
            for c in 0..beds {
                let label = cicp_label(unsafe { sda_bed(c) }).ok_or("MPEG-H unsupported bed geometry; refusing to invent object positions")?;
                let target = if let Some(i) = labels.iter().position(|l| l == label) { i } else { labels.push(label.into()); labels.len()-1 };
                bed_targets.push(target);
            }
            let bed_count = labels.len();
            for id in 0..objects {
                labels.push(format!("Obj_{id}"));
                declarations.push(ObjectChannelDecl { id: id as u32, channel: (bed_count + id as usize) as u32 });
            }
            let pcm = unsafe { slice::from_raw_parts(sda_pcm(), n * (beds + objects) as usize) };
            let mut mapped = vec![vec![0.0; n]; labels.len()];
            for c in 0..(beds + objects) as usize {
                let target = if c < beds as usize { bed_targets[c] } else { bed_count+c-beds as usize };
                for s in 0..n { mapped[target][s] += pcm[c*n+s]; }
            }
            channels = mapped;
            let rows = info(9);
            if !(1..=1024).contains(&rows) { return Err("MPEG-H object PCM has no matching metadata".into()); }
            let metadata = unsafe { slice::from_raw_parts(sda_metadata(), rows as usize*12) };
            for row in metadata.chunks_exact(12) {
                if row.iter().any(|v| !v.is_finite()) || row[0] < 0.0 || row[0] >= objects as f32 || row[1] < 0.0 {
                    return Err("MPEG-H invalid object metadata".into());
                }
                let az = row[2] as f64 * std::f64::consts::PI / 180.0;
                let el = row[3] as f64 * std::f64::consts::PI / 180.0;
                events.push(ObjectEvent {
                    id: row[0] as u32, sample_pos: self.sample_pos + row[1] as u64, has_pos: true,
                    pos: [-az.sin()*el.cos(), az.cos()*el.cos(), el.sin()],
                    gain_db: 20.0*(row[5] as f64).max(1e-10).log10(),
                    size: [(row[6] as f64/180.0).min(1.0), (row[8] as f64).min(1.0), (row[7] as f64/180.0).min(1.0)],
                    diffuse: (row[9] as f64).clamp(0.0,1.0),
                    anchor: if row[10] != 0.0 { "screen" } else { "room" }.into(),
                    // OAM radius is relative, exactly as Windows: never invent metres.
                    distance_m: None, distance_infinite: false, screen_factor: None, depth_factor: None,
                    ramp_duration: row[11] as u32,
                });
            }
        } else {
            labels = vec!["L".into(), "R".into()];
            channels = self.reference_stereo()?;
        }
        let frames = channels[0].len();
        let frame = FrameData { codec: "mpegh", sample_rate: rate as u32, sample_pos: self.sample_pos,
            raw_bed_labels: labels.iter().filter(|l| !l.starts_with("Obj_")).cloned().collect(),
            channels, labels, events, object_channels: declarations, program_loudness: None, ramp_duration: 0 };
        self.sample_pos += frames as u64;
        Ok(Some(frame))
    }
    fn take_speakers(&mut self) -> Result<Option<FrameData>, String> {
        let (count, bytes, rate) = unsafe { (sda_info(11), sda_info(10), sda_info(2)) };
        if bytes == 0 { return Ok(None); }
        if count != 12 || rate != 48000 || bytes < 0 || bytes % 36 != 0 || bytes > 4096*36 {
            return Err(format!("MPEG-H unexpected 7.1.4 output: channels={count}, bytes={bytes}, rate={rate}"));
        }
        let ptr = unsafe { sda_rendered() };
        if ptr.is_null() { return Err("MPEG-H missing speaker PCM".into()); }
        let pcm = unsafe { slice::from_raw_parts(ptr, bytes as usize) };
        let mut channels = vec![Vec::with_capacity(pcm.len()/36); 12];
        for (i, sample) in pcm.chunks_exact(3).enumerate() {
            let v = (sample[0] as i32) | ((sample[1] as i32)<<8) | ((sample[2] as i32)<<16);
            channels[i%12].push(((v<<8)>>8) as f32 / 8388608.0);
        }
        // impeg hd_cicp_2_geometry_rom.c: CICP19 is rear BEFORE side.
        let labels: Vec<String> = ["L","R","C","LFE","Lb","Rb","Ls","Rs","Tfl","Tfr","Tbl","Tbr"].iter().map(|s| s.to_string()).collect();
        let frame = FrameData { codec: "mpegh", sample_rate: rate as u32, sample_pos: self.sample_pos,
            raw_bed_labels: labels.clone(), channels, labels, events: vec![], object_channels: vec![], program_loudness: None, ramp_duration: 0 };
        self.sample_pos += (bytes/36) as u64;
        Ok(Some(frame))
    }
    /// Same upstream PCM24 stereo reference used by Windows's loudness meter.
    pub fn reference_stereo(&self) -> Result<Vec<Vec<f32>>, String> {
        let (count, bytes) = unsafe { (sda_info(11), sda_info(10)) };
        if count != 2 || bytes <= 0 || bytes % 6 != 0 || bytes > 4096*6 { return Err("MPEG-H unexpected stereo output".into()); }
        let pcm = unsafe { slice::from_raw_parts(sda_rendered(), bytes as usize) };
        let mut channels = vec![Vec::with_capacity(pcm.len()/6), Vec::with_capacity(pcm.len()/6)];
        for (i, sample) in pcm.chunks_exact(3).enumerate() {
            let v = (sample[0] as i32) | ((sample[1] as i32)<<8) | ((sample[2] as i32)<<16);
            channels[i%2].push(((v<<8)>>8) as f32 / 8388608.0);
        }
        Ok(channels)
    }
}
impl Drop for MpeghDecoder {
    fn drop(&mut self) { unsafe { sda_close(); } OWNED.store(false, Ordering::Release); }
}
fn cicp_label(c: i32) -> Option<&'static str> {
    Some(match c {
        0=>"L",1=>"R",2=>"C",3|26=>"LFE",4|13=>"Ls",5|14=>"Rs",6=>"Lc",7=>"Rc",8|41=>"Lb",9|42=>"Rb",10=>"Cb",
        15=>"Lw",16=>"Rw",17|32=>"Tfl",18|33=>"Tfr",19=>"Tfc",20|30=>"Tbl",21|31=>"Tbr",22=>"Tbc",23=>"Tsl",24=>"Tsr",25=>"Tc",_=>return None,
    })
}
// MHAS framing only, equivalent to Windows diagnostic-framing.ts. This prevents
// partial access units from corrupting the stateful upstream audio decoder.
fn access_unit(bytes: &[u8], start: usize) -> Result<Option<usize>, String> {
    struct Bits<'a> { bytes: &'a [u8], bit: usize }
    impl Bits<'_> {
        fn get(&mut self,n: usize)->Option<u64> {
            if self.bit+n > self.bytes.len()*8 { return None; }
            let mut value=0;
            for _ in 0..n { value=(value<<1)|((self.bytes[self.bit/8]>>(7-self.bit%8))&1) as u64; self.bit+=1; }
            Some(value)
        }
        fn escaped(&mut self,a: usize,b: usize,c: usize)->Option<u64> {
            let mut v=self.get(a)?;
            if v==(1<<a)-1 { let w=self.get(b)?; v+=w; if w==(1<<b)-1 { v+=self.get(c)?; } }
            Some(v)
        }
    }
    let mut bits=Bits { bytes,bit:start*8 };
    while bits.bit<bytes.len()*8 {
        let Some(kind)=bits.escaped(3,8,8) else { return Ok(None) };
        if bits.escaped(2,8,32).is_none() { return Ok(None); }
        let Some(length)=bits.escaped(11,24,24) else { return Ok(None) };
        if length>MAX_PENDING as u64 { return Err("MPEG-H packet too large".into()); }
        if bits.bit%8 != 0 { return Err("MPEG-H unaligned packet".into()); }
        bits.bit+=length as usize*8;
        if bits.bit>bytes.len()*8 { return Ok(None); }
        if kind==2 { return Ok(Some(bits.bit/8)); }
    }
    Ok(None)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn capture(bytes: &[u8], chunk: usize) -> (Vec<f32>, Vec<String>, Vec<f32>) {
        let mut decoder = MpeghDecoder::new().unwrap();
        assert!(MpeghDecoder::new().is_err(), "singleton cannot acquire a second owner");
        let mut pcm = Vec::new(); let mut events = Vec::new(); let mut references = Vec::new();
        for bytes in bytes.chunks(chunk) {
            decoder.push(bytes).unwrap();
            while let Some((frame, reference)) = decoder.next_frame_with_reference() {
                assert_eq!(reference.len(), 2);
                // Reference PCM includes decoder trimming; source capture uses full access units.
                assert_eq!(reference[0].len(), reference[1].len());
                assert!(!reference[0].is_empty() && reference[0].len() <= 4096);
                references.extend(reference.into_iter().flatten());
                assert_eq!(frame.codec, "mpegh"); assert_eq!(frame.sample_rate, 48000);
                assert_eq!(frame.labels, ["Obj_0", "Obj_1"]);
                assert_eq!(frame.object_channels.len(), 2);
                assert!(!frame.events.is_empty());
                assert!(frame.events.iter().all(|e| e.distance_m.is_none() && !e.distance_infinite));
                for channel in frame.channels { pcm.extend(channel); }
                events.push(serde_json::to_string(&frame.events).unwrap());
            }
        }
        decoder.flush().unwrap();
        (pcm, events, references)
    }
    // A real iPhone crash had a 544 KiB GCD worker stack. This must exercise
    // MHAS config/frame parsing, not just construction or an empty feed. The
    // old ~1 MiB DRC local aborts this process with stack overflow.
    #[test]
    fn mhas_decode_fits_ios_dispatch_stack_without_changing_pcm_or_metadata() {
        let fixture = include_bytes!("../../../packages/core/mpegh/fixtures/motion.mhas");
        let baseline = capture(fixture, fixture.len());
        // MHAS packet type 22, label 0, length 1: zero loudness entries,
        // no album entries or extensions. Exercise the changed branch too.
        let mut with_loudness = vec![0xe1, 0xe0, 0x01, 0x00];
        with_loudness.extend_from_slice(fixture);
        assert!(capture(&with_loudness, 997) == baseline, "loudness packet changes PCM/OAM");
        for layout in [2, 19] {
            let with_loudness = with_loudness.clone();
            let result = std::thread::Builder::new()
                .name("ios-544k-decode-regression".into())
                .stack_size(544 * 1024)
                .spawn(move || {
                    if layout == 2 { return Some(capture(&with_loudness, 997)); }
                    let mut decoder = MpeghDecoder::new_7_1_4().unwrap();
                    let mut samples = 0;
                    for bytes in with_loudness.chunks(997) {
                        decoder.push(bytes).unwrap();
                        while let Some(frame) = decoder.next_frame() {
                            assert_eq!(frame.channels.len(), 12);
                            assert!(frame.channels.iter().flatten().all(|s| s.is_finite()));
                            samples += frame.channels[0].len();
                        }
                    }
                    decoder.flush().unwrap();
                    assert!(samples > 48000);
                    None
                }).unwrap().join().unwrap();
            if let Some(result) = result { assert!(result == baseline, "small-stack PCM/OAM differs"); }
        }
    }
    #[test]
    fn speakers_7_1_4_are_real_chunk_invariant_pcm_and_exclusive() {
        let fixture = include_bytes!("../../../packages/core/mpegh/fixtures/motion.mhas");
        let decode = |chunk| {
            let mut decoder = MpeghDecoder::new_7_1_4().unwrap();
            assert!(MpeghDecoder::new().is_err());
            let mut pcm = vec![];
            for bytes in fixture.chunks(chunk) {
                decoder.push(bytes).unwrap();
                while let Some(frame) = decoder.next_frame() {
                    assert_eq!(frame.labels, ["L","R","C","LFE","Lb","Rb","Ls","Rs","Tfl","Tfr","Tbl","Tbr"]);
                    assert_eq!(frame.sample_rate, 48000);
                    assert!(frame.events.is_empty() && frame.object_channels.is_empty());
                    assert_eq!(frame.channels.len(), 12);
                    assert!(frame.channels.iter().all(|c| c.len() == frame.channels[0].len()));
                    pcm.extend(frame.channels.into_iter().flatten());
                }
            }
            decoder.flush().unwrap();
            pcm
        };
        let baseline = decode(fixture.len());
        assert!(!baseline.is_empty() && baseline.iter().all(|s| s.is_finite()));
        assert!(baseline.iter().any(|s| s.abs() > 0.00001));
        for chunk in [1,7,1024] { assert_eq!(decode(chunk), baseline); }
        let mut decoder = MpeghDecoder::new_7_1_4().unwrap();
        decoder.push(&fixture[..fixture.len()-1]).unwrap();
        assert!(decoder.flush().is_err());
    }
    #[test]
    fn windows_fixture_is_chunk_invariant_with_exclusive_ownership_and_strict_eof() {
        let fixture = include_bytes!("../../../packages/core/mpegh/fixtures/motion.mhas");
        let baseline = capture(fixture, fixture.len());
        assert!(!baseline.0.is_empty());
        for chunk in [1, 7, 1024] { assert_eq!(capture(fixture, chunk), baseline); }
        let mut decoder = MpeghDecoder::new().unwrap();
        decoder.push(&fixture[..fixture.len()-1]).unwrap();
        assert!(decoder.flush().unwrap_err().contains("truncated"));
        drop(decoder);
        let mut decoder = MpeghDecoder::new().unwrap();
        decoder.push(fixture).unwrap();
        decoder.flush().unwrap();
    }
}
