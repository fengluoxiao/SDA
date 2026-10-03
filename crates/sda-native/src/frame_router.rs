//! Desktop player source mapping and decoder-worker event compaction.
use std::collections::BTreeMap;
use sda_core::{FrameData, ObjectEvent};
use crate::{Command, EngineResult, native_object_event, render_command::RenderCommand};

#[derive(Default)]
pub(crate) struct FrameRouter {
    objects: BTreeMap<u32, usize>,
    declared: BTreeMap<String, Option<String>>,
    targets: BTreeMap<u32, ObjectEvent>,
}

fn same_target(a: &ObjectEvent, b: &ObjectEvent) -> bool {
    a.has_pos == b.has_pos && a.pos == b.pos && a.gain_db == b.gain_db
        && a.size == b.size && a.diffuse == b.diffuse && a.anchor == b.anchor && a.distance_m == b.distance_m
        && a.distance_infinite == b.distance_infinite && a.screen_factor == b.screen_factor
        && a.depth_factor == b.depth_factor
}

impl FrameRouter {
    pub fn route(&mut self, frame: FrameData) -> EngineResult<Vec<RenderCommand>> {
        let mut mapping = BTreeMap::new();
        let has_objects = frame.labels.iter().any(|s| s.starts_with("Obj_"));
        if !frame.object_channels.is_empty() {
            for entry in &frame.object_channels {
                if entry.channel as usize >= frame.channels.len()
                    || mapping.values().any(|&ch| ch == entry.channel as usize)
                    || mapping.insert(entry.id, entry.channel as usize).is_some() {
                    return Err("invalid object-to-channel declaration".into());
                }
            }
        } else if has_objects {
            for (ch, label) in frame.labels.iter().enumerate() {
                if let Some(id) = label.strip_prefix("Obj_").and_then(|s| s.parse().ok()) {
                    mapping.insert(id, ch);
                }
            }
        }
        if frame.labels.len() != frame.channels.len() {
            return Err("PCM channel labels do not match channels".into());
        }
        let mut commands = Vec::new();
        // A sparse declaration replaces the mapping; all-bed frames retire it.
        if !mapping.is_empty() || !has_objects {
            for id in self.objects.keys().filter(|id| !mapping.contains_key(id)) {
                let source = format!("obj:{id}");
                commands.push(RenderCommand::Command(Command::RemoveSource {
                    id: source.clone(), at: frame.sample_pos,
                }));
                self.declared.remove(&source);
                self.targets.remove(id);
            }
            self.objects = mapping;
        }
        let mut entries = Vec::with_capacity(frame.channels.len());
        for (ch, samples) in frame.channels.into_iter().enumerate() {
            let object = self.objects.iter().find_map(|(&id, &channel)| (channel == ch).then_some(id));
            let id = object.map_or_else(|| format!("bed:{ch}"), |id| format!("obj:{id}"));
            let bed = object.is_none().then(|| frame.labels[ch].clone());
            if self.declared.get(&id) != Some(&bed) {
                commands.push(RenderCommand::Command(Command::AddSource {
                    id: id.clone(), at: Some(frame.sample_pos), bed_label: bed.clone(),
                }));
                self.declared.insert(id.clone(), bed);
            }
            entries.push((id, samples));
        }
        if !has_objects { self.targets.clear(); }
        let mut events = Vec::new();
        for event in frame.events {
            let redundant = self.targets.get(&event.id).is_some_and(|previous| {
                let ramp = if previous.ramp_duration == 0 { 128 } else { previous.ramp_duration };
                same_target(previous, &event)
                    && previous.sample_pos.saturating_add(u64::from(ramp)) <= event.sample_pos
            });
            if !redundant {
                events.push(native_object_event(&event)?);
                self.targets.insert(event.id, event);
            }
        }
        commands.push(RenderCommand::PcmFrame { start: frame.sample_pos, entries, events });
        Ok(commands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sda_core::ObjectChannelDecl;
    use crate::NativeObjectEvent;

    fn frame(at: u64, labels: &[&str], mapping: &[(u32, u32)]) -> FrameData {
        FrameData { codec: "eac3", sample_rate: 48000, sample_pos: at,
            channels: labels.iter().enumerate().map(|(i,_)| vec![i as f32 + 1.0; 16]).collect(),
            labels: labels.iter().map(|s| s.to_string()).collect(), raw_bed_labels: vec![],
            events: vec![], object_channels: mapping.iter().map(|&(id,channel)| ObjectChannelDecl {id,channel}).collect(),
            program_loudness: None, ramp_duration: 0 }
    }
    fn event(at: u64) -> ObjectEvent {
        ObjectEvent { diffuse: 0.0, id: 42, sample_pos: at, has_pos: true, pos: [0.0,1.0,0.0],
            gain_db: 0.0, size: [0.0;3], anchor: "room".into(), distance_m: None,
            distance_infinite: false, screen_factor: None, depth_factor: None, ramp_duration: 128 }
    }
    #[test]
    fn codec_mapping_wins_over_labels_and_retirement_uses_codec_clock() {
        let mut router = FrameRouter::default();
        let commands = router.route(frame(1000, &["L", "Obj_0"], &[(42,1)])).unwrap();
        let RenderCommand::PcmFrame { entries, .. } = commands.last().unwrap() else { panic!() };
        assert_eq!(entries, &vec![("bed:0".into(),vec![1.0;16]),("obj:42".into(),vec![2.0;16])]);
        let commands = router.route(frame(1016, &["L", "R"], &[])).unwrap();
        assert!(commands.iter().any(|c| matches!(c, RenderCommand::Command(Command::RemoveSource { id, at: 1016 }) if id=="obj:42")));
        let RenderCommand::PcmFrame {entries,..}=commands.last().unwrap() else {panic!()};
        assert_eq!(entries[1].0,"bed:1");
        assert!(router.objects.is_empty());
    }
    #[test]
    fn repeated_targets_are_removed_only_after_the_previous_ramp() {
        let mut router=FrameRouter::default();
        for (at, count) in [(0,1),(64,1),(192,0),(256,0)] {
            let mut f=frame(at,&["Obj_42"],&[]); f.events=vec![event(at)];
            let commands=router.route(f).unwrap();
            let RenderCommand::PcmFrame {events,..}=commands.last().unwrap() else {panic!()};
            assert_eq!(events.len(),count,"sample {at}");
        }
        router.route(frame(300,&["L"],&[])).unwrap();
        let mut f=frame(400,&["Obj_42"],&[]); f.events=vec![event(400)];
        let commands=router.route(f).unwrap();
        let RenderCommand::PcmFrame {events,..}=commands.last().unwrap() else {panic!()};
        assert_eq!(events.len(),1,"a reintroduced object needs its initial metadata");
    }
    #[test]
    fn rejects_ambiguous_mapping_without_changing_state() {
        let mut router=FrameRouter::default();
        assert!(router.route(frame(0,&["Obj_1"],&[(1,0),(2,0)])).is_err());
        assert!(router.objects.is_empty());
        assert!(router.declared.is_empty());
    }

    #[test]
    fn distance_updates_survive_routing_and_use_the_desktop_wire_contract() {
        let mut router = FrameRouter::default();
        for (i, (distance, infinite, has_pos)) in [
            (Some(0.35), false, true), (Some(2.5), false, false),
            (None, true, true), (None, false, true),
        ].into_iter().enumerate() {
            let at = i as u64 * 1536;
            let mut e = event(at);
            e.distance_m = distance;
            e.distance_infinite = infinite;
            e.has_pos = has_pos;
            e.gain_db = -3.5;
            e.size = [0.25, 0.5, 0.75];
            let expected = NativeObjectEvent::from_decoder_contract(42, at, has_pos,
                [0.0, 1.0, 0.0], -3.5, [0.25, 0.5, 0.75], distance.map(|v| v as f32), infinite, 128);
            let mut f = frame(at, &["Obj_42"], &[]);
            f.events.push(e);
            let commands = router.route(f).unwrap();
            let RenderCommand::PcmFrame {events, ..} = commands.last().unwrap() else {panic!()};
            assert_eq!(events, &[expected], "distance transition {i} was lost or changed");
        }
    }
}
