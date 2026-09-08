//! TrueHD (MLP, incl. Atmos) streaming pipeline — a WASM-facing port of
//! harletty-bridge's `bridge/src/truehd_pipeline.rs` +
//! `bridge/src/metadata.rs::build_metadata_frame_from_oamd`.
//!
//! Flow: `Extractor` (re-frames raw bytes) → `Parser` (access units) →
//! `Decoder::decode_presentation` (PCM + OAMD). Presentation 3 (the full
//! 16-channel Atmos presentation) is requested, matching the harletty CLI
//! default; the parser's substream mask degrades gracefully on streams that
//! carry fewer presentations.

use std::collections::VecDeque;

use truehd::process::{decode::Decoder, extract::Extractor, parse::Parser};
use truehd::structs::oamd::ObjectAudioMetadataPayload;

use crate::{FrameData, ObjectChannelDecl, ObjectEvent, Pipeline, ProgramLoudnessMetadata};

/// Full Atmos presentation (harletty CLI default).
const PRESENTATION: usize = 3;

fn truehd_dialogue_level(encoded: u8) -> i8 {
    if encoded == 0 {
        -31
    } else {
        -(encoded as i8)
    }
}

#[derive(Clone, Copy)]
struct TruehdDialogueNorms {
    two: u8,
    six: u8,
    eight: u8,
    sixteen: Option<u8>,
}

fn truehd_dialogue_level_for_output(
    norms: TruehdDialogueNorms,
    channel_count: usize,
    has_oamd: bool,
) -> i8 {
    let encoded = if has_oamd || channel_count > 8 {
        norms.sixteen.unwrap_or(norms.eight)
    } else if channel_count <= 2 {
        norms.two
    } else if channel_count <= 6 {
        norms.six
    } else {
        norms.eight
    };
    truehd_dialogue_level(encoded)
}

fn update_spatial_labels(
    spatial_labels: &mut Option<Vec<String>>,
    labels: &[String],
    declarations: &[ObjectChannelDecl],
    has_oamd: bool,
    supported: bool,
) {
    if !has_oamd {
        return;
    }
    if !supported || declarations.is_empty() {
        *spatial_labels = None;
        return;
    }
    let mut next = labels.to_vec();
    for declaration in declarations {
        if let Some(label) = next.get_mut(declaration.channel as usize) {
            *label = format!("Obj_{}", declaration.id);
        }
    }
    *spatial_labels = Some(next);
}

pub struct TruehdPipeline {
    extractor: Extractor,
    parser: Parser,
    decoder: Decoder,
    total_samples: u64,
    declared: Option<Vec<ObjectChannelDecl>>,
    spatial_labels: Option<Vec<String>>,
    dialogue_norms: Option<TruehdDialogueNorms>,
    program_loudness: Option<ProgramLoudnessMetadata>,
}

impl TruehdPipeline {
    pub fn new() -> Self {
        let mut parser = Parser::default();
        parser.set_required_presentations(&[true, true, true, true]);
        Self {
            extractor: Extractor::default(),
            parser,
            decoder: Decoder::default(),
            total_samples: 0,
            declared: None,
            spatial_labels: None,
            dialogue_norms: None,
            program_loudness: None,
        }
    }

    fn sparse_declare(&mut self, current: Vec<ObjectChannelDecl>) -> Vec<ObjectChannelDecl> {
        if self.declared.as_ref() == Some(&current) {
            Vec::new()
        } else {
            self.declared = Some(current.clone());
            current
        }
    }
}

impl Pipeline for TruehdPipeline {
    fn codec_name(&self) -> &'static str {
        "truehd"
    }

    fn push(&mut self, data: &[u8], out: &mut VecDeque<FrameData>, errors: &mut Vec<String>) {
        self.extractor.push_bytes(data);
        loop {
            let frame = match self.extractor.next() {
                Some(Ok(frame)) => frame,
                Some(Err(e)) => {
                    // InsufficientData just means "need more bytes"; anything
                    // else is a resync, which the extractor handles internally.
                    if !matches!(e, truehd::utils::errors::ExtractError::InsufficientData) {
                        errors.push(format!("TrueHD extract error: {e}"));
                    }
                    break;
                }
                None => break,
            };

            let access_unit = match self.parser.parse(&frame) {
                Ok(au) => au,
                Err(e) => {
                    errors.push(format!("TrueHD parse error: {e}"));
                    continue;
                }
            };

            if let Some(major_sync) = &access_unit.major_sync_info {
                let channel_meaning = &major_sync.channel_meaning;
                self.dialogue_norms = Some(TruehdDialogueNorms {
                    two: channel_meaning.twoch_dialogue_norm,
                    six: channel_meaning.sixch_dialogue_norm,
                    eight: channel_meaning.eightch_dialogue_norm,
                    sixteen: channel_meaning
                        .extra_channel_meaning
                        .as_ref()
                        .map(|extra| extra.sixteench_dialogue_norm),
                });
            }

            let decoded = match self.decoder.decode_presentation(&access_unit, PRESENTATION) {
                Ok(d) => d,
                Err(e) => {
                    errors.push(format!("TrueHD decode error: {e}"));
                    continue;
                }
            };

            // Duplicate AUs carry no new audio time — discard (CLI parity).
            if decoded.is_duplicate {
                continue;
            }

            if let Some(norms) = self.dialogue_norms {
                let dialogue_level_db = truehd_dialogue_level_for_output(
                    norms,
                    decoded.channel_count,
                    !decoded.oamd.is_empty(),
                );
                self.program_loudness = Some(ProgramLoudnessMetadata::dolby(
                    "truehd-dialogue-norm",
                    dialogue_level_db,
                ));
            }

            let sample_pos = self.total_samples;
            self.total_samples += decoded.sample_length as u64;

            // Planar f32 from sample-major 24-bit-in-i32 PCM.
            let channel_count = decoded.channel_count.min(16);
            let mut channels: Vec<Vec<f32>> =
                vec![Vec::with_capacity(decoded.sample_length); channel_count];
            for row in decoded.pcm_data.iter().take(decoded.sample_length) {
                for (ch, slot) in channels.iter_mut().enumerate().take(channel_count) {
                    slot.push(row[ch] as f32 / 8_388_608.0);
                }
            }

            let mut labels: Vec<String> = decoded
                .channel_labels
                .iter()
                .take(channel_count)
                .map(|l| format!("{l:?}"))
                .collect();
            while labels.len() < channel_count {
                labels.push(format!("Ch{}", labels.len()));
            }

            if decoded.substream_info_changed {
                self.declared = None;
                self.spatial_labels = None;
            }
            let mut events = Vec::new();
            let mut declarations = Vec::new();
            let mut unsupported_oamd = false;
            for oamd in &decoded.oamd {
                let extracted = extract_events(oamd, sample_pos + oamd.evo_sample_offset);
                if !extracted.supported {
                    unsupported_oamd = true;
                    break;
                }
                declarations.extend(extracted.objects.iter().map(|&(id, channel)| {
                    ObjectChannelDecl {
                        id,
                        channel: channel as u32,
                    }
                }));
                events.extend(extracted.events);
            }
            if unsupported_oamd {
                events.clear();
                declarations.clear();
                self.declared = None;
            }
            update_spatial_labels(
                &mut self.spatial_labels,
                &labels,
                &declarations,
                !decoded.oamd.is_empty(),
                !unsupported_oamd,
            );
            if let Some(spatial_labels) = &self.spatial_labels {
                labels = spatial_labels.clone();
            }
            events.sort_by_key(|event| event.sample_pos);
            let object_channels = if decoded.oamd.is_empty() {
                Vec::new()
            } else {
                self.sparse_declare(declarations)
            };

            let ramp_duration = decoded
                .oamd
                .first()
                .and_then(|o| o.object_element.as_ref())
                .and_then(|e| e.md_update_info.block_update_info.first())
                .map(|b| b.ramp_duration as u32)
                .unwrap_or(0);

            out.push_back(FrameData {
                codec: "truehd",
                sample_rate: decoded.sampling_frequency,
                sample_pos,
                channels,
                labels: labels.clone(),
                raw_bed_labels: labels
                    .into_iter()
                    .filter(|label| !label.starts_with("Obj_"))
                    .collect(),
                events,
                object_channels,
                program_loudness: self.program_loudness.clone(),
                ramp_duration,
            });
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

/// Port of `bridge/src/metadata.rs::extract_events` (TrueHD OAMD).
/// Bed members contribute no events here — their channels are fixed and
/// described by the frame labels; only dynamic objects emit events.
struct Extracted {
    events: Vec<ObjectEvent>,
    /// Dynamic-object declaration: (id, PCM channel index).
    objects: Vec<(u32, usize)>,
    supported: bool,
}

fn extract_events(oamd: &ObjectAudioMetadataPayload, base_sample_pos: u64) -> Extracted {
    let unsupported = Extracted {
        events: Vec::new(),
        objects: Vec::new(),
        supported: false,
    };

    let object_count = oamd.object_count;
    let Some(object_element) = &oamd.object_element else {
        return unsupported;
    };
    // Unsupported multi-block / multi-bed / ISF layouts: skip metadata
    // (audio still plays as a flat bed), same policy as the bridge.
    if object_element.md_update_info.num_obj_info_blocks != 1
        || oamd.program_assignment.bed_assignment.len() != 1
        || oamd.program_assignment.num_isf_objects != 0
    {
        return unsupported;
    }

    let sample_offset = object_element.md_update_info.sample_offset as u64;
    let ramp_duration = object_element.md_update_info.block_update_info[0].ramp_duration as u32;
    let sample_pos = base_sample_pos + sample_offset;

    let pos_vec = oamd.get_damf_pos();
    let bed_index_vec = oamd
        .program_assignment
        .bed_assignment
        .first()
        .map(|b| b.to_index_vec())
        .unwrap_or_default();

    let mut events = Vec::with_capacity(object_count);
    let mut objects = Vec::new();

    for i in 0..object_count {
        let Some(object_blocks) = object_element.object_data.get(i) else {
            continue;
        };
        let Some(object_data) = object_blocks.first() else {
            continue;
        };
        if object_data.b_object_in_bed_or_isf {
            continue;
        }

        let id = (i + 10 - bed_index_vec.len()) as u32;
        let render = &object_data.object_render_info;
        let (has_pos, pos, size) = match pos_vec.get(i).and_then(|blocks| blocks.first()) {
            Some(raw) if raw.len() >= 3 => (true, [raw[0], raw[1], raw[2]], render.object_size),
            _ => (false, [0.0; 3], [0.0; 3]),
        };
        let (distance_m, distance_infinite) = match render.distance_factor {
            Some(distance) if distance.is_infinite() => (None, true),
            Some(distance) => (Some(distance), false),
            None => (None, false),
        };
        let anchor = if object_data.b_object_in_bed_or_isf {
            "speaker"
        } else if render.b_object_use_screen_ref {
            "screen"
        } else {
            "room"
        }
        .to_string();

        objects.push((id, i));
        events.push(ObjectEvent {
            id,
            sample_pos,
            has_pos,
            pos,
            gain_db: f64::from(object_data.object_basic_info.object_gain),
            size,
            anchor,
            distance_m,
            distance_infinite,
            screen_factor: render
                .b_object_use_screen_ref
                .then_some(render.screen_factor),
            depth_factor: render
                .b_object_use_screen_ref
                .then_some(render.depth_factor),
            ramp_duration,
        });
    }

    Extracted {
        events,
        objects,
        supported: true,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        truehd_dialogue_level, truehd_dialogue_level_for_output, update_spatial_labels,
        TruehdDialogueNorms,
    };
    use crate::ObjectChannelDecl;

    #[test]
    fn dialogue_norm_zero_code_maps_to_minus_31() {
        assert_eq!(truehd_dialogue_level(0), -31);
        assert_eq!(truehd_dialogue_level(27), -27);
    }

    #[test]
    fn dialogue_norm_tracks_the_decoded_presentation() {
        let norms = TruehdDialogueNorms {
            two: 20,
            six: 21,
            eight: 22,
            sixteen: Some(23),
        };
        assert_eq!(truehd_dialogue_level_for_output(norms, 2, false), -20);
        assert_eq!(truehd_dialogue_level_for_output(norms, 6, false), -21);
        assert_eq!(truehd_dialogue_level_for_output(norms, 8, false), -22);
        assert_eq!(truehd_dialogue_level_for_output(norms, 16, false), -23);
        assert_eq!(truehd_dialogue_level_for_output(norms, 6, true), -23);
    }

    #[test]
    fn unsupported_oamd_clears_cached_object_labels() {
        let labels = vec!["L".to_string(), "R".to_string()];
        let mut spatial = Some(vec!["Obj_10".to_string(), "R".to_string()]);
        update_spatial_labels(&mut spatial, &labels, &[], true, false);
        assert!(spatial.is_none());
    }

    #[test]
    fn absent_oamd_preserves_sparse_object_labels() {
        let labels = vec!["L".to_string(), "R".to_string()];
        let mut spatial = Some(vec!["Obj_10".to_string(), "R".to_string()]);
        update_spatial_labels(&mut spatial, &labels, &[], false, true);
        assert_eq!(spatial.unwrap()[0], "Obj_10");
    }

    #[test]
    fn supported_oamd_replaces_object_labels() {
        let labels = vec!["L".to_string(), "R".to_string()];
        let declarations = vec![ObjectChannelDecl { id: 12, channel: 1 }];
        let mut spatial = None;
        update_spatial_labels(&mut spatial, &labels, &declarations, true, true);
        assert_eq!(spatial.unwrap(), vec!["L", "Obj_12"]);
    }
}
