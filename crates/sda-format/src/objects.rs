//! Versioned independent object PCM mapping and sample-clock automation.
use crate::metadata::Metadata;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectEvent {
    pub id: u32,
    pub sample_pos: u64,
    pub has_pos: bool,
    pub pos: [f64; 3],
    pub gain_db: f64,
    pub size: [f64; 3],
    pub anchor: String,
    #[serde(default)]
    pub diffuse: f64,
    pub distance_m: Option<f64>,
    #[serde(default)]
    pub distance_infinite: bool,
    pub screen_factor: Option<f64>,
    pub depth_factor: Option<f64>,
    pub ramp_duration: u32,
    #[serde(default)]
    pub zone_exclusion: Vec<serde_json::Value>,
    #[serde(default)]
    pub horizontal_only: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ObjectChannel {
    pub id: u32,
    pub channel: u32,
}
#[derive(Default, Debug)]
pub struct ObjectTimeline {
    pub channels: Vec<ObjectChannel>,
    events: Vec<ObjectEvent>,
    next: usize,
    end: Option<u64>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Timeline {
    schema: String,
    version: u32,
    sample_rate: u32,
    duration_samples: u64,
    events: Vec<ObjectEvent>,
}
impl ObjectTimeline {
    pub fn parse(metadata: &Metadata) -> Result<Self, String> {
        let scene = &metadata.render["scene"];
        if scene["kind"] == "bed" {
            return Ok(Self::default());
        }
        if scene["kind"] != "bed-objects" {
            return Err("Unsupported object scene".into());
        }
        let channels = scene["channels"].as_array().ok_or("Missing source lanes")?;
        let mut mapping = vec![];
        let mut ids = BTreeSet::new();
        let mut lanes = BTreeSet::new();
        for object in scene["objects"]
            .as_array()
            .ok_or("Missing object declarations")?
        {
            let id = object["id"]
                .as_u64()
                .filter(|v| *v <= u32::MAX as u64)
                .ok_or("Invalid object id")? as u32;
            let lane = object["pcmChannel"]
                .as_u64()
                .filter(|v| *v > 0 && *v <= channels.len() as u64)
                .ok_or("Invalid object PCM lane")? as usize
                - 1;
            if !ids.insert(id)
                || !lanes.insert(lane)
                || channels[lane]["kind"] != "object"
                || channels[lane]["objectId"] != id
                || channels[lane]["name"] != format!("Obj_{id}")
                || channels[lane]["sourceId"] != format!("obj:{id}")
                || object["sourceId"] != format!("obj:{id}")
            {
                return Err("Conflicting object identity / PCM mapping".into());
            }
            mapping.push(ObjectChannel {
                id,
                channel: lane as u32,
            });
        }
        if mapping.is_empty()
            || channels.iter().filter(|c| c["kind"] == "object").count() != mapping.len()
        {
            return Err("Unassigned object PCM lanes".into());
        }
        let descriptor = &scene["automation"];
        if descriptor["schema"] != "sda.object-timeline"
            || descriptor["version"] != 1
            || descriptor["encoding"] != "zlib-json"
            || descriptor["sampleRate"] != 48000
        {
            return Err("Unsupported SDA object timeline contract".into());
        }
        let asset = metadata.asset(
            descriptor["asset"]
                .as_str()
                .ok_or("Missing object timeline asset")?,
        )?;
        let raw = crate::lossless::inflate_metadata(&asset.data)?;
        let timeline: Timeline =
            serde_json::from_slice(&raw).map_err(|e| format!("Invalid object timeline: {e}"))?;
        if timeline.schema != "sda.object-timeline"
            || timeline.version != 1
            || timeline.sample_rate != 48000
            || timeline.duration_samples != metadata.master_frames()
            || descriptor["eventCount"].as_u64() != Some(timeline.events.len() as u64)
            || timeline.events.len() > 2_000_000
        {
            return Err("Conflicting object timeline".into());
        }
        let mut previous = None;
        let mut present = BTreeSet::new();
        for e in &timeline.events {
            if !ids.contains(&e.id)
                || e.sample_pos > metadata.master_frames()
                || previous.is_some_and(|p| p > (e.sample_pos, e.id))
                || !e.gain_db.is_finite()
                || !(-200.0..=24.0).contains(&e.gain_db)
                || e.pos.iter().any(|v| !v.is_finite() || v.abs() > 4.0)
                || e.size
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                || !e.diffuse.is_finite()
                || !(0.0..=1.0).contains(&e.diffuse)
                || e.anchor != "room"
                || !e.zone_exclusion.is_empty()
                || e.horizontal_only
                || e.distance_m.is_some_and(|v| !v.is_finite() || v < 0.0)
                || [e.screen_factor, e.depth_factor]
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite())
            {
                return Err("Unsupported or invalid SDA object event".into());
            }
            previous = Some((e.sample_pos, e.id));
            if present.insert(e.id) && e.sample_pos != 0 {
                return Err("Object must declare its initial state at sample zero".into());
            }
        }
        if present != ids {
            return Err("Object lacks initial automation state".into());
        }
        Ok(Self {
            channels: mapping,
            events: timeline.events,
            next: 0,
            end: None,
        })
    }
    /// Pull only events in this PCM interval. A discontinuity reconstructs the
    /// interrupted ramp from its original endpoints, then continues its remainder.
    pub fn events(&mut self, start: u64, frames: usize) -> Vec<ObjectEvent> {
        let end = start.saturating_add(frames as u64);
        let mut result = vec![];
        if self.end != Some(start) {
            self.next = self.events.partition_point(|e| e.sample_pos < start);
            let mut states: BTreeMap<u32, (ObjectEvent, ObjectEvent)> = BTreeMap::new();
            for e in &self.events[..self.next] {
                let from = if let Some((from, target)) = states.get(&e.id) {
                    interpolate(from, target, e.sample_pos)
                } else {
                    e.clone()
                };
                states.insert(e.id, (from, e.clone()));
            }
            for (_, (from, target)) in states {
                let mut current = interpolate(&from, &target, start);
                current.sample_pos = start;
                current.ramp_duration = 0;
                result.push(current);
                let target_end = target
                    .sample_pos
                    .saturating_add(target.ramp_duration as u64);
                if target_end > start {
                    let mut remainder = target;
                    remainder.sample_pos = start;
                    remainder.ramp_duration = (target_end - start) as u32;
                    result.push(remainder);
                }
            }
        }
        while self.next < self.events.len() && self.events[self.next].sample_pos < end {
            result.push(self.events[self.next].clone());
            self.next += 1;
        }
        result.sort_by_key(|e| (e.sample_pos, e.id));
        self.end = Some(end);
        result
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}
fn interpolate(from: &ObjectEvent, target: &ObjectEvent, sample: u64) -> ObjectEvent {
    let t = if target.ramp_duration == 0 {
        1.0
    } else {
        ((sample.saturating_sub(target.sample_pos)) as f64 / target.ramp_duration as f64).min(1.0)
    };
    let mut v = target.clone();
    for i in 0..3 {
        v.size[i] = from.size[i] + (target.size[i] - from.size[i]) * t;
    }
    v.diffuse = from.diffuse + (target.diffuse - from.diffuse) * t;
    if target.has_pos {
        for i in 0..3 {
            v.pos[i] = from.pos[i] + (target.pos[i] - from.pos[i]) * t;
        }
    } else {
        v.pos = from.pos;
        v.has_pos = true;
    }
    let a = 10f64.powf(from.gain_db / 20.0);
    let b = 10f64.powf(target.gain_db / 20.0);
    v.gain_db = 20.0 * (a + (b - a) * t).max(1e-10).log10();
    v
}
#[cfg(test)]
mod tests {
    use super::*;
    fn event(sample: u64, x: f64, ramp: u32) -> ObjectEvent {
        ObjectEvent {
            id: 7,
            sample_pos: sample,
            has_pos: true,
            pos: [x, 1.0, 0.0],
            gain_db: 0.0,
            size: [0.0; 3],
            anchor: "room".into(),
            diffuse: 0.0,
            distance_m: None,
            distance_infinite: false,
            screen_factor: None,
            depth_factor: None,
            ramp_duration: ramp,
            zone_exclusion: vec![],
            horizontal_only: false,
        }
    }
    #[test]
    fn seek_recovers_interrupted_ramp_and_does_not_replay_past_movement() {
        let mut t = ObjectTimeline {
            events: vec![event(0, -1.0, 0), event(100, 1.0, 100), event(250, -1.0, 0)],
            ..Default::default()
        };
        let v = t.events(150, 32);
        assert_eq!(v.len(), 2);
        assert!((v[0].pos[0]).abs() < 1e-12);
        assert_eq!(v[1].ramp_duration, 50);
        assert_eq!(v[1].sample_pos, 150);
        assert!(t.events(182, 32).is_empty());
        assert_eq!(t.events(214, 40)[0].sample_pos, 250);
        let v = t.events(50, 20);
        assert_eq!(v[0].pos[0], -1.0);
    }
}
