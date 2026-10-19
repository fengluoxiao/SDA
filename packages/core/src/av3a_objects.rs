//! Audio Vivid Mix transport-to-scene mapping. Transport channels are not speakers.
use crate::{ObjectChannelDecl, ObjectEvent};
use avs3a::metadata::{
    DynamicMetadata, DynamicObjectExtent, DynamicObjectMetadata, DynamicObjectPosition,
    FrameMetadata, MetadataGain, MetadataGainUnit, StaticMetadata,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Role {
    Bed(&'static str),
    Object(u32),
}
#[derive(Clone, Debug)]
struct Input {
    transport: usize,
    gain: f32,
}
#[derive(Clone, Debug, PartialEq)]
struct Target {
    pos: [f64; 3],
    size: [f64; 3],
    gain: f32,
    diffuse: f64,
    muted: bool,
    jump: bool,
}
#[derive(Clone, Debug)]
struct Route {
    role: Role,
    inputs: Vec<Input>,
    last: Option<Target>,
}
#[derive(Clone, Debug)]
struct ObjectSpec {
    index: u8,
    packs: Vec<u8>,
    gain: f32,
    head_locked: bool,
}
#[derive(Clone, Debug)]
struct PackSpec {
    index: u8,
    kind: u8,
    format: Option<u8>,
    start: Option<u8>,
    channels: Vec<(u8, Option<u8>)>,
}
#[derive(Clone, Debug)]
struct SceneSpec {
    objects: Vec<ObjectSpec>,
    packs: Vec<PackSpec>,
    gains: BTreeMap<u8, f32>,
}
fn gain(value: Option<MetadataGain>) -> Result<f32, String> {
    let Some(value) = value else { return Ok(1.) };
    let result = match value.unit {
        MetadataGainUnit::Linear => value.value,
        MetadataGainUnit::Decibels => 10f32.powf(value.value / 20.),
    };
    if !result.is_finite() || result < 0. {
        return Err("invalid AV3A static gain".into());
    }
    Ok(result)
}
fn spec(meta: &StaticMetadata) -> Result<SceneSpec, String> {
    if meta.basic_level != 0 {
        return Err("unsupported AV3A basic metadata level".into());
    }
    let basic = &meta.basic;
    let mut active = BTreeSet::new();
    for reference in &basic.programme.content_references {
        let content = basic
            .contents
            .iter()
            .find(|c| c.index == *reference)
            .ok_or("missing AV3A content reference")?;
        active.extend(content.object_references.iter().copied());
    }
    if active.is_empty() {
        return Err("AV3A scene has no active object references".into());
    }
    let objects = basic
        .objects
        .iter()
        .filter(|o| active.contains(&o.index))
        .map(|o| {
            Ok(ObjectSpec {
                index: o.index,
                packs: o.pack_references.as_slice().to_vec(),
                gain: if o.muted { 0. } else { gain(o.gain)? },
                head_locked: o.head_locked,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if objects.len() != active.len() {
        return Err("missing AV3A object reference".into());
    }
    let packs = basic
        .packs
        .iter()
        .map(|p| PackSpec {
            index: p.index,
            kind: p.type_label,
            format: p.pack_format_id,
            start: p.pack_format_start_index,
            channels: p
                .channels
                .iter()
                .map(|c| (c.channel_index, c.transformed_channel_reference))
                .collect(),
        })
        .collect();
    let gains = basic
        .channels
        .iter()
        .map(|c| Ok((c.index, gain(c.gain)?)))
        .collect::<Result<_, String>>()?;
    Ok(SceneSpec {
        objects,
        packs,
        gains,
    })
}
// ADM common-definition pack IDs, not AV3A channel-configuration indices.
fn speakers(id: u8) -> Result<&'static [&'static str], String> {
    match id {
        1 => Ok(&["C"]),
        2 => Ok(&["L", "R"]),
        3 => Ok(&["L", "R", "C", "LFE", "Ls", "Rs"]),
        15 => Ok(&["L", "R", "C", "LFE", "Ls", "Rs", "Lb", "Rb"]),
        _ => Err(format!("unsupported AV3A direct-speaker pack {id}")),
    }
}
fn compile(scene: SceneSpec, channels: usize) -> Result<Vec<Route>, String> {
    let mut beds: BTreeMap<&'static str, Vec<Input>> = BTreeMap::new();
    let mut objects = BTreeMap::new();
    for object in scene.objects {
        if object.head_locked {
            return Err("AV3A head-locked scene is not supported".into());
        }
        for pack_id in object.packs {
            let pack = scene
                .packs
                .iter()
                .find(|p| p.index == pack_id)
                .ok_or("missing AV3A pack reference")?;
            let labels = if pack.kind == 1 {
                Some(speakers(
                    pack.format.ok_or("missing AV3A speaker pack ID")?,
                )?)
            } else if pack.kind == 3 {
                None
            } else {
                return Err(format!("unsupported AV3A pack type {}", pack.kind));
            };
            if labels.is_some_and(|l| l.len() != pack.channels.len()) {
                return Err("AV3A speaker pack count mismatch".into());
            }
            for (i, (channel, transport)) in pack.channels.iter().enumerate() {
                let transport = transport
                    .map(usize::from)
                    .or_else(|| pack.start.map(|p| usize::from(p) + i))
                    .ok_or("missing AV3A transport mapping")?;
                if transport >= channels {
                    return Err("AV3A transport reference out of range".into());
                }
                let g = object.gain
                    * scene
                        .gains
                        .get(channel)
                        .copied()
                        .ok_or("missing AV3A channel gain definition")?;
                if !g.is_finite() {
                    return Err("invalid combined AV3A gain".into());
                }
                let input = Input { transport, gain: g };
                if let Some(labels) = labels {
                    beds.entry(labels[i]).or_default().push(input);
                } else {
                    let id =
                        1 + u32::from(object.index) * 256 + u32::from(pack.index) * 32 + i as u32;
                    if objects
                        .insert(
                            id,
                            Route {
                                role: Role::Object(id),
                                inputs: vec![input],
                                last: None,
                            },
                        )
                        .is_some()
                    {
                        return Err("duplicate AV3A logical channel".into());
                    }
                }
            }
        }
    }
    let mut routes = Vec::new();
    for label in ["L", "R", "C", "LFE", "Ls", "Rs", "Lb", "Rb"] {
        if let Some(inputs) = beds.remove(label) {
            routes.push(Route {
                role: Role::Bed(label),
                inputs,
                last: None,
            });
        }
    }
    routes.extend(objects.into_values());
    if routes.is_empty() || routes.len() > 64 {
        return Err("unsupported AV3A logical scene size".into());
    }
    Ok(routes)
}
fn target(object: &DynamicObjectMetadata, previous: Option<&Target>) -> Result<Target, String> {
    if object.muted {
        let mut state = previous.cloned().unwrap_or(Target {
            pos: [0.; 3],
            size: [0.; 3],
            gain: 0.,
            diffuse: 0.,
            muted: true,
            jump: true,
        });
        state.gain = 0.;
        state.muted = true;
        state.jump = true;
        return Ok(state);
    }
    let l = object
        .level1
        .as_ref()
        .ok_or("missing AV3A dynamic object position")?;
    if let Some(extra) = object.level2 {
        if extra.channel_lock.is_some_and(|l| l.locked)
            || extra.object_divergence.is_some_and(|d| d.divergence != 0.)
            || extra.object_screen_reference == Some(true)
            || extra.screen_edge_lock.is_some_and(|v| v != 0)
        {
            return Err("unsupported AV3A object-lock/divergence/screen metadata".into());
        }
    }
    let pos = match l.position {
        DynamicObjectPosition::Cartesian { x, y, z } => [f64::from(x), f64::from(y), f64::from(z)],
        DynamicObjectPosition::Polar {
            azimuth,
            elevation,
            distance,
        } => {
            let (a, e, r) = (
                f64::from(azimuth).to_radians(),
                f64::from(elevation).to_radians(),
                f64::from(distance),
            );
            [-r * e.cos() * a.sin(), r * e.cos() * a.cos(), r * e.sin()]
        }
    };
    let size = match l.extent {
        None => [0.; 3],
        Some(DynamicObjectExtent::Cartesian {
            width_x,
            height_y,
            depth_z,
        }) => [f64::from(width_x), f64::from(height_y), f64::from(depth_z)],
        Some(DynamicObjectExtent::Polar {
            width,
            height,
            depth,
        }) if width == 0. && height == 0. && depth == 0. => [0.; 3],
        Some(DynamicObjectExtent::Polar { .. }) => {
            return Err("AV3A polar extent conversion is not validated".into())
        }
    };
    let gain = l.gain.unwrap_or(1.);
    let diffuse = f64::from(l.diffuse.unwrap_or(0.));
    if !pos.iter().chain(&size).all(|v| v.is_finite())
        || size.iter().any(|v| !(0.0..=1.0).contains(v))
        || !gain.is_finite()
        || !(0.0..=6.0).contains(&gain)
        || !diffuse.is_finite()
        || !(0.0..=1.0).contains(&diffuse)
    {
        return Err("invalid AV3A object metadata value".into());
    }
    Ok(Target {
        pos,
        size,
        gain,
        diffuse,
        muted: false,
        jump: l.jump_position,
    })
}
#[derive(Clone)]
pub(crate) struct ObjectScene {
    channels: usize,
    routes: Vec<Route>,
    targets: Vec<Option<Target>>,
    previous_gain: Vec<Option<f32>>,
    started: bool,
}
pub(crate) struct SceneFrame {
    pub channels: Vec<Vec<f32>>,
    pub labels: Vec<String>,
    pub beds: Vec<String>,
    pub declarations: Vec<ObjectChannelDecl>,
    pub events: Vec<ObjectEvent>,
}
impl ObjectScene {
    pub fn new(channels: usize) -> Self {
        Self {
            channels,
            routes: vec![],
            targets: vec![None; channels],
            previous_gain: vec![None; channels],
            started: false,
        }
    }
    pub fn transport_channels(&self) -> usize {
        self.channels
    }
    pub fn reset_dynamic(&mut self) {
        self.targets.fill(None);
        self.previous_gain.fill(None);
        self.started = false;
        for r in &mut self.routes {
            r.last = None;
        }
    }
    fn install(&mut self, scene: SceneSpec) -> Result<(), String> {
        let mut routes = compile(scene, self.channels)?;
        if self.started {
            let key = |r: &Route| {
                (
                    r.role.clone(),
                    r.inputs.iter().map(|i| i.transport).collect::<Vec<_>>(),
                )
            };
            if self.routes.iter().map(key).collect::<Vec<_>>()
                != routes.iter().map(key).collect::<Vec<_>>()
            {
                return Err("AV3A logical topology changed during playback".into());
            }
            for (old, new) in self.routes.iter().zip(&mut routes) {
                new.last = old.last.clone();
            }
        }
        self.routes = routes;
        Ok(())
    }
    fn update(&mut self, dynamic: &DynamicMetadata) -> Result<(), String> {
        self.update_objects(dynamic.objects.as_slice())
    }
    fn update_objects(&mut self, objects: &[DynamicObjectMetadata]) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        let mut updates = Vec::new();
        for object in objects {
            let channel = usize::from(object.transport_channel_reference);
            if channel >= self.channels || !seen.insert(channel) {
                return Err("invalid/duplicate AV3A dynamic transport reference".into());
            }
            updates.push((channel, target(object, self.targets[channel].as_ref())?));
        }
        for (channel, target) in updates {
            self.targets[channel] = Some(target);
        }
        Ok(())
    }
    pub fn process(
        &mut self,
        pcm: Vec<Vec<f32>>,
        meta: &FrameMetadata,
        at: u64,
        duration: u32,
    ) -> Result<SceneFrame, String> {
        if let Some(s) = meta.static_metadata() {
            self.install(spec(s)?)?;
        }
        if let Some(d) = meta.dynamic_metadata() {
            self.update(d)?;
        }
        self.route(pcm, at, duration)
    }
    fn route(&mut self, pcm: Vec<Vec<f32>>, at: u64, duration: u32) -> Result<SceneFrame, String> {
        if self.routes.is_empty() {
            return Err("AV3A Mix requires initial static transport-to-scene metadata".into());
        }
        if pcm.len() != self.channels
            || pcm.is_empty()
            || pcm.iter().any(|p| p.len() != pcm[0].len())
        {
            return Err("AV3A transport PCM shape mismatch".into());
        }
        let count = pcm[0].len();
        let mut frame = SceneFrame {
            channels: Vec::new(),
            labels: Vec::new(),
            beds: Vec::new(),
            declarations: Vec::new(),
            events: Vec::new(),
        };
        for route in &mut self.routes {
            let mut output = vec![0.; count];
            match route.role {
                Role::Bed(label) => {
                    frame.labels.push(label.into());
                    frame.beds.push(label.into());
                    for input in &route.inputs {
                        let gain = self.targets[input.transport]
                            .as_ref()
                            .map_or(1., |t| t.gain);
                        let previous = self.previous_gain[input.transport].unwrap_or(gain);
                        for (i, (out, sample)) in
                            output.iter_mut().zip(&pcm[input.transport]).enumerate()
                        {
                            let g = if gain == 0. {
                                0.
                            } else {
                                previous + (gain - previous) * (i + 1) as f32 / count as f32
                            };
                            *out += sample * input.gain * g;
                        }
                    }
                }
                Role::Object(id) => {
                    let input = &route.inputs[0];
                    let target = self.targets[input.transport]
                        .as_ref()
                        .ok_or("AV3A object has no initial dynamic position")?;
                    for (out, sample) in output.iter_mut().zip(&pcm[input.transport]) {
                        *out = if target.gain == 0. || target.muted {
                            0.
                        } else {
                            sample * input.gain
                        };
                    }
                    frame.declarations.push(ObjectChannelDecl {
                        id,
                        channel: frame.channels.len() as u32,
                    });
                    frame.labels.push(format!("Obj_{id}"));
                    if route.last.as_ref() != Some(target) {
                        let initial = route.last.as_ref().is_none_or(|t| t.muted);
                        frame.events.push(ObjectEvent {
                            id,
                            sample_pos: at,
                            has_pos: !target.muted,
                            pos: target.pos,
                            gain_db: if target.gain > 0. {
                                20. * f64::from(target.gain).log10()
                            } else {
                                -120.
                            },
                            size: target.size,
                            anchor: "room".into(),
                            diffuse: target.diffuse,
                            distance_m: None,
                            distance_infinite: false,
                            screen_factor: None,
                            depth_factor: None,
                            ramp_duration: if initial || target.jump || target.muted {
                                1
                            } else {
                                duration.max(1)
                            },
                        });
                        route.last = Some(target.clone());
                    }
                }
            }
            if !output.iter().all(|v| v.is_finite()) {
                return Err("non-finite routed AV3A PCM".into());
            }
            frame.channels.push(output);
        }
        for (i, target) in self.targets.iter().enumerate() {
            self.previous_gain[i] = Some(target.as_ref().map_or(1., |t| t.gain));
        }
        self.started = true;
        Ok(frame)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use avs3a::metadata::{DynamicLevel1Metadata, DynamicObjectPosition};
    fn scene() -> ObjectScene {
        let mut s = ObjectScene::new(3);
        let gains = (0..8)
            .map(|i| (i, if i < 6 && i != 3 { 0. } else { 1. }))
            .collect();
        s.install(SceneSpec {
            objects: vec![
                ObjectSpec {
                    index: 0,
                    packs: vec![0],
                    gain: 1.,
                    head_locked: false,
                },
                ObjectSpec {
                    index: 1,
                    packs: vec![1],
                    gain: 1.,
                    head_locked: false,
                },
            ],
            packs: vec![
                PackSpec {
                    index: 0,
                    kind: 1,
                    format: Some(3),
                    start: None,
                    channels: (0..6).map(|i| (i, Some(0))).collect(),
                },
                PackSpec {
                    index: 1,
                    kind: 3,
                    format: None,
                    start: None,
                    channels: vec![(6, Some(1)), (7, Some(2))],
                },
            ],
            gains,
        })
        .unwrap();
        s
    }
    fn object(channel: u8, pos: [f32; 3], gain: f32) -> DynamicObjectMetadata {
        DynamicObjectMetadata {
            transport_channel_reference: channel,
            level1: Some(DynamicLevel1Metadata {
                position: DynamicObjectPosition::Cartesian {
                    x: pos[0],
                    y: pos[1],
                    z: pos[2],
                },
                extent: None,
                gain: Some(gain),
                diffuse: None,
                jump_position: false,
                importance: None,
            }),
            ..Default::default()
        }
    }
    #[test]
    fn av3a_mix_expands_reused_transport_into_lfe_and_independent_objects() {
        let mut s = scene();
        s.update_objects(&[
            object(0, [0.; 3], 1.),
            object(1, [-1., 1., 0.], 0.5),
            object(2, [1., 1., 0.], 1.),
        ])
        .unwrap();
        let f = s
            .route(vec![vec![1.; 4], vec![2.; 4], vec![3.; 4]], 0, 1024)
            .unwrap();
        assert_eq!(f.channels.len(), 8);
        assert_eq!(f.beds, ["L", "R", "C", "LFE", "Ls", "Rs"]);
        for ch in [0, 1, 2, 4, 5] {
            assert_eq!(f.channels[ch], [0.; 4]);
        }
        assert_eq!(f.channels[3], [1.; 4]);
        assert_eq!(f.channels[6], [2.; 4]);
        assert_eq!(f.channels[7], [3.; 4]);
        assert_eq!(
            f.declarations,
            vec![
                ObjectChannelDecl {
                    id: 289,
                    channel: 6
                },
                ObjectChannelDecl {
                    id: 290,
                    channel: 7
                }
            ]
        );
        assert_eq!(f.events[0].pos, [-1., 1., 0.]);
        assert_eq!(f.events[1].pos, [1., 1., 0.]);
        assert!((f.events[0].gain_db + 6.020599913).abs() < 1e-6);
        assert_eq!(f.events[0].ramp_duration, 1);
        assert!(f.events.iter().all(|e| e.distance_m.is_none()));
        let repeated = s.route(vec![vec![1.; 4]; 3], 1024, 1024).unwrap();
        assert!(repeated.events.is_empty());
    }
    #[test]
    fn av3a_mix_mute_motion_clock_and_reset_do_not_invent_sources() {
        let mut s = scene();
        let mut objects = vec![
            object(0, [0.; 3], 1.),
            object(1, [-1., 1., 0.], 1.),
            object(2, [1., 1., 0.], 1.),
        ];
        s.update_objects(&objects).unwrap();
        s.route(vec![vec![0.1; 8]; 3], 0, 1024).unwrap();
        objects[1] = object(1, [-0.5, 1., 0.25], 0.25);
        s.update_objects(&objects).unwrap();
        let f = s.route(vec![vec![0.1; 8]; 3], 1115, 1114).unwrap();
        assert_eq!(f.events.len(), 1);
        assert_eq!(f.events[0].sample_pos, 1115);
        assert_eq!(f.events[0].ramp_duration, 1114);
        assert_eq!(f.events[0].pos, [-0.5, 1., 0.25]);
        objects[1].muted = true;
        objects[1].level1 = None;
        s.update_objects(&objects).unwrap();
        let f = s.route(vec![vec![0.1; 8]; 3], 2229, 1115).unwrap();
        assert_eq!(f.channels[6], [0.; 8]);
        assert_eq!(f.events[0].gain_db, -120.);
        assert!(!f.events[0].has_pos);
        s.reset_dynamic();
        s.update_objects(&[
            object(0, [0.; 3], 1.),
            object(1, [-1., 1., 0.], 1.),
            object(2, [1., 1., 0.], 1.),
        ])
        .unwrap();
        let f = s.route(vec![vec![0.1; 8]; 3], 0, 1024).unwrap();
        assert_eq!(f.channels.len(), 8);
        assert!(f
            .events
            .iter()
            .all(|e| e.sample_pos == 0 && e.ramp_duration == 1));
    }
    #[test]
    fn av3a_mix_rejects_ambiguous_or_unpositioned_inputs() {
        let mut s = scene();
        assert!(s
            .update_objects(&[object(1, [0.; 3], 1.), object(1, [0.; 3], 1.)])
            .is_err());
        assert!(s.update_objects(&[object(3, [0.; 3], 1.)]).is_err());
        assert!(s.route(vec![vec![0.; 8]; 3], 0, 1024).is_err());
        assert!(ObjectScene::new(3)
            .route(vec![vec![0.; 8]; 3], 0, 1024)
            .is_err());
        assert_eq!(
            gain(Some(MetadataGain {
                unit: MetadataGainUnit::Linear,
                value: 0.
            }))
            .unwrap(),
            0.
        );
        assert!(
            (gain(Some(MetadataGain {
                unit: MetadataGainUnit::Decibels,
                value: -6.
            }))
            .unwrap()
                - 0.5011872)
                .abs()
                < 1e-6
        );
        assert!(gain(Some(MetadataGain {
            unit: MetadataGainUnit::Linear,
            value: -1.
        }))
        .is_err());
    }
    #[test]
    fn av3a_mix_polar_direction_is_adm_positive_left() {
        let mut o = object(1, [0.; 3], 1.);
        o.level1.as_mut().unwrap().position = DynamicObjectPosition::Polar {
            azimuth: 90.,
            elevation: 0.,
            distance: 0.5,
        };
        let t = target(&o, None).unwrap();
        assert!((t.pos[0] + 0.5).abs() < 1e-10);
        assert!(t.pos[1].abs() < 1e-10);
    }
}
