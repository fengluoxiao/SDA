//! DTS core / DTS-HD MA / DTS:X, including transmitted object metadata.
//! Unsupported folds retain their compatible bed contribution and emit silence
//! on the independent feed, so unverified content is never rendered twice.
use crate::{FrameData, ObjectChannelDecl, ObjectEvent, Pipeline};
use dca::{
    FoldPlan, HdDecoder, HdError, HdFrame, SourceRole, SpatialChannel, XMetadata, XPresentation,
};
use std::collections::VecDeque;

const CORE: [u8; 4] = 0x7ffe8001u32.to_be_bytes();
const EXSS: [u8; 4] = 0x64582025u32.to_be_bytes();
const HEIGHT_GAIN: f32 = 23170.0 / 32768.0;

pub struct DtsPipeline {
    buffer: Vec<u8>,
    pcm_decoder: dca::PcmDecoder,
    hd_decoder: HdDecoder,
    total_samples: u64,
    locked: Option<XPresentation>,
    warned_extension: bool,
}

// Read only the bounded EXSS size header. The full dca parser requires the
// entire substream and cannot distinguish a partial packet from an invalid one.
fn exss_size(data: &[u8]) -> Option<Result<usize, &'static str>> {
    if data.len() < 10 {
        return None;
    }
    let bits = |start: usize, len: usize| -> usize {
        (start..start + len).fold(0, |v, b| {
            (v << 1) | ((data[b / 8] >> (7 - b % 8)) & 1) as usize
        })
    };
    let wide = bits(42, 1);
    let header = bits(43, 8 + 4 * wide) + 1;
    let size = bits(51 + 4 * wide, 16 + 4 * wide) + 1;
    Some(if header < 10 || size < header {
        Err("invalid EXSS size")
    } else {
        Ok(size)
    })
}

fn frame(
    sample_rate: u32,
    sample_pos: u64,
    channels: Vec<Vec<f32>>,
    labels: Vec<String>,
) -> FrameData {
    FrameData {
        codec: "dts",
        sample_rate,
        sample_pos,
        channels,
        raw_bed_labels: labels.clone(),
        labels,
        events: Vec::new(),
        object_channels: Vec::new(),
        program_loudness: None,
        ramp_duration: 0,
    }
}

fn hd_frame(
    hd: HdFrame,
    sample_pos: u64,
    locked: &mut Option<XPresentation>,
) -> Result<FrameData, &'static str> {
    let metadata = XMetadata::parse(&hd.x_payload, hd.x_samples.len()).ok();
    render_hd(hd, metadata, sample_pos, locked)
}

fn spatial_label(channel: SpatialChannel) -> &'static str {
    match channel {
        SpatialChannel::TopFrontLeft => "Tfl",
        SpatialChannel::TopFrontRight => "Tfr",
        SpatialChannel::TopBackLeft => "Tbl",
        SpatialChannel::TopBackRight => "Tbr",
        SpatialChannel::TopFrontCenter => "Tfc",
        SpatialChannel::TopSideLeft => "Tsl",
        SpatialChannel::TopSideRight => "Tsr",
        SpatialChannel::WideLeft => "Lw",
        SpatialChannel::WideRight => "Rw",
    }
}

fn object_feeds(presentation: XPresentation) -> std::ops::Range<usize> {
    // D0 declares a real object and also permits a fixed centre-height
    // alternative. SDA has an object renderer, so keep its transmitted position.
    if presentation == XPresentation::FixedD0 {
        0..1
    } else {
        presentation.object_feeds()
    }
}

fn render_hd(
    hd: HdFrame,
    metadata: Option<XMetadata>,
    sample_pos: u64,
    locked: &mut Option<XPresentation>,
) -> Result<FrameData, &'static str> {
    let n = hd.bed_sample_count();
    if n == 0 || hd.sample_rate == 0 || hd.samples.iter().flatten().any(|c| c.len() != n) {
        return Err("invalid DTS-HD PCM dimensions");
    }
    let detected = XPresentation::detect(&hd);
    if let Some(p) = detected {
        *locked = Some(p);
    }
    // Never reuse stale coordinates or folds after an unreadable metadata frame.
    // Retain the channel shape but keep undecodable content in the compatible bed.
    let metadata =
        metadata.filter(|m| detected.is_some() && m.source_count() == hd.x_samples.len());
    let plan = match (detected, metadata.as_ref()) {
        (Some(_), Some(m)) => FoldPlan::from_metadata(m),
        (Some(XPresentation::Height), None)
            if [1, 2, 7, 8]
                .iter()
                .all(|&i| hd.samples.get(i).is_some_and(Option::is_some)) =>
        {
            FoldPlan::standard_heights(HEIGHT_GAIN)
        }
        _ => FoldPlan::all_unknown(locked.map_or(0, XPresentation::feed_count)),
    };
    let names = ["C", "L", "R", "Ls", "Rs", "LFE", "Cb", "Lb", "Rb"];
    let mut channels = Vec::new();
    let mut labels = Vec::new();
    for (i, samples) in hd.samples.iter().enumerate() {
        if let Some(samples) = samples {
            let label = names
                .get(i)
                .ok_or("unsupported DTS-HD speaker assignment")?;
            channels.push(
                samples
                    .iter()
                    .enumerate()
                    .map(|(s, &v)| plan.clean(i, v, s, &hd.x_samples))
                    .collect(),
            );
            labels.push((*label).to_string());
        }
    }
    let mut declarations = Vec::new();
    let mut events = Vec::new();
    if let Some(p) = *locked {
        let feed_pcm = |feed: usize| -> Vec<f32> {
            if detected.is_some() && plan.source_is_known(feed) {
                hd.x_samples[feed].clone()
            } else {
                vec![0.0; n]
            }
        };
        let objects = object_feeds(p);
        let fixed = if p == XPresentation::FixedD0 {
            XPresentation::Height.fixed_channels()
        } else {
            p.fixed_channels()
        };
        for (feed, &label) in (objects.end..p.feed_count()).zip(fixed) {
            channels.push(feed_pcm(feed));
            labels.push(spatial_label(label).into());
        }
        for feed in objects {
            let id = 1000 + feed as u32;
            declarations.push(ObjectChannelDecl {
                id,
                channel: channels.len() as u32,
            });
            channels.push(feed_pcm(feed));
            labels.push(format!("Obj_{id}"));
            if let Some(SourceRole::Object { position, .. }) = metadata
                .as_ref()
                .and_then(|m| m.source(feed))
                .map(|s| s.role)
            {
                events.push(ObjectEvent {
            diffuse: 0.0,
                    id,
                    sample_pos,
                    has_pos: true,
                    pos: position.to_adm_cartesian(),
                    gain_db: 0.0,
                    size: [0.0; 3],
                    anchor: "room".into(),
                    distance_m: None,
                    distance_infinite: false,
                    screen_factor: None,
                    depth_factor: None,
                    ramp_duration: 0,
                });
            }
        }
    }
    let mut result = frame(hd.sample_rate, sample_pos, channels, labels);
    result
        .raw_bed_labels
        .truncate(result.labels.len() - declarations.len());
    result.object_channels = declarations;
    result.events = events;
    Ok(result)
}

impl DtsPipeline {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            pcm_decoder: dca::PcmDecoder::new(),
            hd_decoder: HdDecoder::new(),
            total_samples: 0,
            locked: None,
            warned_extension: false,
        }
    }
    fn drain(&mut self, eof: bool, out: &mut VecDeque<FrameData>, errors: &mut Vec<String>) {
        let mut used = 0;
        loop {
            let rest = &self.buffer[used..];
            let Some(offset) = rest.windows(4).position(|w| w == CORE) else {
                used += rest.len().saturating_sub(if eof { 0 } else { 3 });
                break;
            };
            used += offset;
            let rest = &self.buffer[used..];
            let info = match dca::parse_header(rest) {
                Ok(info) => info,
                Err(dca::HeaderParseError::InsufficientData) => break,
                Err(_) => {
                    used += 4;
                    continue;
                }
            };
            let core_len = info.frame_size;
            if rest.len() < core_len {
                break;
            }
            if rest.len() < core_len + 4 && !eof {
                break;
            }
            let mut size = core_len;
            let has_exss = rest.get(core_len..core_len + 4) == Some(&EXSS);
            if has_exss {
                match exss_size(&rest[core_len..]) {
                    None => break,
                    Some(Err(e)) => {
                        errors.push(crate::diagnostics::failure("dts.failure_01", e.into()));
                        used += core_len + 4;
                        continue;
                    }
                    Some(Ok(n)) => size += n,
                }
                if rest.len() < size {
                    break;
                }
            }
            let decoded = if has_exss && dca::exss_has_xll(&rest[core_len..size]) {
                match self
                    .hd_decoder
                    .decode(&rest[..core_len], &rest[core_len..size])
                {
                    Ok(hd) => {
                        if (hd.x_present || hd.x_imax)
                            && XMetadata::parse(&hd.x_payload, hd.x_samples.len()).is_err()
                            && !self.warned_extension
                        {
                            errors.push(crate::diagnostics::failure("dts.failure_02", "DTS:X metadata unsupported or corrupt: using compatible bed for unknown folds".into()));
                            self.warned_extension = true;
                        }
                        hd_frame(hd, self.total_samples, &mut self.locked)
                            .map(Some)
                            .map_err(str::to_string)
                    }
                    Err(HdError::Pending) => Ok(None),
                    Err(e) => Err(format!("DTS-HD decode error: {e:?}")),
                }
            } else {
                self.pcm_decoder
                    .push_access_unit(&rest[..core_len])
                    .map(|r| {
                        let mut channels = r.pcm.fullband_channels;
                        let mut labels: Vec<String> = r
                            .pcm
                            .fullband_channel_order
                            .iter()
                            .map(|b| format!("{b:?}"))
                            .collect();
                        if let Some(lfe) = r.pcm.lfe_channel {
                            channels.push(lfe);
                            labels.push("LFE".into());
                        }
                        Some(frame(
                            r.pcm.sample_rate,
                            self.total_samples,
                            channels,
                            labels,
                        ))
                    })
                    .map_err(|e| format!("DTS decode error: {e}"))
            };
            match decoded {
                Ok(Some(f)) => {
                    self.total_samples += f.channels.first().map_or(0, Vec::len) as u64;
                    out.push_back(f);
                }
                Ok(None) => (),
                Err(e) => errors.push(crate::diagnostics::failure("dts.failure_03", e)),
            }
            used += size;
        }
        self.buffer.drain(..used);
        if eof && !self.buffer.is_empty() {
            errors.push(crate::diagnostics::failure("dts.failure_04", "truncated DTS frame at end of stream".into()));
            self.buffer.clear();
        }
    }
}
impl Pipeline for DtsPipeline {
    fn codec_name(&self) -> &'static str {
        "dts"
    }
    fn push(&mut self, data: &[u8], out: &mut VecDeque<FrameData>, errors: &mut Vec<String>) {
        // Bound pending input independently of caller chunk size.
        for chunk in data.chunks(64 * 1024) {
            self.buffer.extend_from_slice(chunk);
            self.drain(false, out, errors);
        }
    }
    fn flush(&mut self, out: &mut VecDeque<FrameData>, errors: &mut Vec<String>) {
        self.drain(true, out, errors);
    }
    fn reset(&mut self) {
        *self = Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn d0_preserves_the_declared_object_instead_of_its_fixed_alternative() {
        use dca::{BedFold, SourceMetadata, SphericalPosition, REFERENCE_CHANNELS};
        let mut source = hd();
        source.x_imax = true;
        source.x_samples = vec![vec![0.1; 16]; 5];
        let position = SphericalPosition {
            azimuth_half_degrees: 0,
            elevation_half_degrees: 51,
            distance_64ths: 64,
        };
        let mut sources = vec![SourceMetadata {
            role: SourceRole::Object {
                position,
                centre_height_alternative: true,
            },
            fold: BedFold::Known([0.0; REFERENCE_CHANNELS]),
        }];
        sources.extend(
            XPresentation::Height
                .fixed_channels()
                .iter()
                .map(|&c| SourceMetadata {
                    role: SourceRole::Height(c),
                    fold: BedFold::Known([0.0; REFERENCE_CHANNELS]),
                }),
        );
        let f = render_hd(source, XMetadata::from_sources(&sources), 0, &mut None).unwrap();
        assert_eq!(f.object_channels.len(), 1);
        assert_eq!(f.object_channels[0].channel, 12);
        assert_eq!(&f.labels[8..12], &["Tfl", "Tfr", "Tbl", "Tbr"]);
        assert_eq!(f.events[0].pos, position.to_adm_cartesian());
        assert_eq!(f.channels[12], vec![0.1; 16]);
    }
    #[test]
    fn transmitted_objects_follow_coordinates_and_pcm_after_fixed_heights() {
        use dca::{BedFold, SourceMetadata, SphericalPosition, REFERENCE_CHANNELS};
        let mut locked = None;
        for (tick, azimuth) in [-180, 0, 180].into_iter().enumerate() {
            let mut source = hd();
            source.x_imax = true;
            source.x_samples = (0..8).map(|i| vec![0.01 * (i + 1) as f32; 16]).collect();
            let sources: Vec<_> = (0..8)
                .map(|i| {
                    let role = if i < 4 {
                        SourceRole::Object {
                            position: SphericalPosition {
                                azimuth_half_degrees: azimuth,
                                elevation_half_degrees: 0,
                                distance_64ths: 64,
                            },
                            centre_height_alternative: false,
                        }
                    } else {
                        SourceRole::Height(XPresentation::Height.fixed_channels()[i - 4])
                    };
                    let mut gains = [0.0; REFERENCE_CHANNELS];
                    gains[0] = 0.5;
                    SourceMetadata {
                        role,
                        fold: BedFold::Known(gains),
                    }
                })
                .collect();
            let metadata = XMetadata::from_sources(&sources).unwrap();
            let folded_speaker = metadata.reference_speakers()[0] as usize;
            let original: f32 = source.x_samples.iter().map(|v| v[0] * 0.5).sum();
            source.samples[folded_speaker] = Some(vec![original; 16]);
            let f = render_hd(source, Some(metadata), tick as u64 * 16, &mut locked).unwrap();
            assert_eq!(f.channels.len(), 16);
            assert_eq!(&f.labels[8..12], &["Tfl", "Tfr", "Tbl", "Tbr"]);
            assert_eq!(f.object_channels.len(), 4);
            assert_eq!(f.events.len(), 4);
            for (index, declaration) in f.object_channels.iter().enumerate() {
                assert_eq!(declaration.channel, 12 + index as u32);
                assert_eq!(f.channels[12 + index], vec![0.01 * (index + 1) as f32; 16]);
                assert_eq!(f.events[index].id, declaration.id);
                assert_eq!(f.events[index].sample_pos, tick as u64 * 16);
                assert!(
                    (f.events[index].pos[0] - (azimuth as f64 / 2.0).to_radians().sin()).abs()
                        < 1e-12
                );
            }
            let bed = metadata.reference_speakers()[0] as usize;
            let bed_index = if bed > 6 { bed - 1 } else { bed };
            assert!(f.channels[bed_index].iter().all(|v| v.abs() < 1e-7));
        }
        // A lost extension keeps the same declaration, but no invented position
        // and no second copy of the objects' compatible-bed contribution.
        let dropout = hd_frame(hd(), 48, &mut locked).unwrap();
        assert_eq!(dropout.object_channels.len(), 4);
        assert!(dropout.events.is_empty());
        assert!(dropout.channels[8..].iter().flatten().all(|s| *s == 0.0));
    }
    #[test]
    fn exss_size_reads_split_and_wide_headers() {
        for wide in [0, 1] {
            let mut bytes = [0u8; 10];
            bytes[..4].copy_from_slice(&EXSS);
            let mut write = |start: usize, len: usize, value: usize| {
                for bit in 0..len {
                    bytes[(start + bit) / 8] |=
                        (((value >> (len - 1 - bit)) & 1) as u8) << (7 - (start + bit) % 8);
                }
            };
            write(42, 1, wide);
            write(43, 8 + 4 * wide, 31);
            write(51 + 4 * wide, 16 + 4 * wide, 60000);
            for n in 0..10 {
                assert!(exss_size(&bytes[..n]).is_none());
            }
            assert_eq!(exss_size(&bytes), Some(Ok(60001)));
        }
    }
    /// Supply a local raw core+EXSS stream. Kept explicit/ignored so a missing
    /// external media fixture cannot masquerade as a successful decode test.
    #[test]
    #[ignore = "requires SDA_DTS_TEST_FILE raw DTS-HD fixture"]
    fn real_hd_stream_is_chunk_invariant() {
        let bytes = std::fs::read(std::env::var("SDA_DTS_TEST_FILE").unwrap()).unwrap();
        let decode = |size| {
            let mut p = DtsPipeline::new();
            let mut out = VecDeque::new();
            let mut errors = Vec::new();
            for chunk in bytes.chunks(size) {
                p.push(chunk, &mut out, &mut errors);
            }
            p.flush(&mut out, &mut errors);
            assert!(
                errors
                    .iter()
                    .all(|e| e == "truncated DTS frame at end of stream"),
                "{errors:?}"
            );
            assert!(out.len() > 100, "not enough decoded frames");
            let mut pos = 0;
            for f in &out {
                assert_eq!(f.sample_pos, pos);
                pos += f.channels[0].len() as u64;
                assert!(f.channels.iter().flatten().all(|s| s.is_finite()));
            }
            eprintln!(
                "chunk={size}, frames={}, samples={pos}, labels={:?}",
                out.len(),
                out[0].labels
            );
            out
        };
        let whole = decode(bytes.len());
        for size in [1, 137, 4096] {
            let split = decode(size);
            assert_eq!(whole.len(), split.len());
            for (a, b) in whole.iter().zip(split.iter()) {
                assert_eq!(a.labels, b.labels);
                assert_eq!(a.channels, b.channels);
            }
        }
    }
    fn hd() -> HdFrame {
        HdFrame {
            sample_rate: 48000,
            samples: (0..9)
                .map(|i| if i == 6 { None } else { Some(vec![0.5; 16]) })
                .collect(),
            ..Default::default()
        }
    }
    #[test]
    fn height_unfold_preserves_original_fold_and_never_fabricates_objects() {
        let mut source = hd();
        source.x_samples = (0..4).map(|i| vec![0.1 * (i + 1) as f32; 16]).collect();
        let mut locked = None;
        let f = hd_frame(source, 512, &mut locked).unwrap();
        assert_eq!(f.channels.len(), 12);
        assert_eq!(f.sample_pos, 512);
        assert!(f.events.is_empty() && f.object_channels.is_empty());
        for (h, label) in ["L", "R", "Lb", "Rb"].iter().enumerate() {
            let bed = f.labels.iter().position(|s| s == label).unwrap();
            for s in 0..16 {
                assert!(
                    (f.channels[bed][s] + HEIGHT_GAIN * f.channels[8 + h][s] - 0.5).abs() < 1e-7
                );
            }
        }
        let fallback = hd_frame(hd(), 528, &mut locked).unwrap();
        assert_eq!(fallback.labels, f.labels);
        assert_eq!(fallback.channels[1], vec![0.5; 16]);
        assert!(fallback.channels[8..].iter().flatten().all(|s| *s == 0.0));
    }
    #[test]
    fn alternate_profiles_retain_unmodified_bed() {
        for count in [5, 6, 8] {
            let mut source = hd();
            source.x_imax = true;
            source.x_samples = vec![vec![0.2; 16]; count];
            let f = hd_frame(source, 0, &mut None).unwrap();
            assert_eq!(f.channels.len(), 8 + count);
            assert!(f.channels[..8].iter().flatten().all(|s| *s == 0.5));
            assert!(f.channels[8..].iter().flatten().all(|s| *s == 0.0));
            assert!(f.events.is_empty());
        }
    }
    #[test]
    fn malformed_pcm_is_rejected_and_reset_clears_layout() {
        let mut source = hd();
        source.samples[0] = Some(vec![0.0]);
        assert!(hd_frame(source, 0, &mut None).is_err());
        let mut p = DtsPipeline::new();
        p.locked = Some(XPresentation::Height);
        p.total_samples = 99;
        p.buffer.extend(CORE);
        p.reset();
        assert!(p.locked.is_none());
        assert_eq!(p.total_samples, 0);
        assert!(p.buffer.is_empty());
    }
    #[test]
    fn garbage_input_is_bounded() {
        let mut p = DtsPipeline::new();
        let mut out = VecDeque::new();
        let mut errors = Vec::new();
        p.push(&vec![0; 2_000_000], &mut out, &mut errors);
        assert!(p.buffer.len() <= 3);
        p.flush(&mut out, &mut errors);
        assert!(p.buffer.is_empty());
    }
}
