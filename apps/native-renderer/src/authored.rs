use super::*;
use serde::Deserialize;
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Processing {
    pub enabled: bool,
    pub version: u32,
    pub programme_handled: bool,
    pub source_spatial_handled: bool,
    pub spatial_presentation: bool,
    pub stereo_source: bool,
    pub programme_gain_db: f32,
    pub makeup_db: f32,
    pub bed_positions: Vec<BedPosition>,
    pub muted_sources: Vec<String>,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BedPosition {
    pub id: String,
    pub position: [f32; 3],
    #[serde(default)]
    pub gain_db: f32,
}
impl Processing {
    fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        if self.version != 1
            || !self.programme_gain_db.is_finite()
            || !(-120.0..=24.0).contains(&self.programme_gain_db)
            || !self.makeup_db.is_finite()
            || !(-60.0..=24.0).contains(&self.makeup_db)
            || self.bed_positions.len() > MAX_SOURCES
        {
            return Err("unsupported authored processing parameters".into());
        }
        if self.muted_sources.iter().any(|id| {
            !id.strip_prefix("bed:").is_some_and(|id|id.parse::<usize>().is_ok_and(|n|n<MAX_SOURCES))
                && !id.strip_prefix("obj:").is_some_and(|id|id.parse::<u32>().is_ok())
        }) {
            return Err("invalid authored source mute".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for ch in &self.bed_positions {
            if !ch.gain_db.is_finite()
                || !(-120.0..=24.0).contains(&ch.gain_db)
                || !ch
                    .id
                    .strip_prefix("bed:")
                    .is_some_and(|id| id.parse::<usize>().is_ok_and(|n| n < MAX_SOURCES))
                || !ids.insert(&ch.id)
                || ch.position.iter().any(|v| !v.is_finite())
                || ch.position.iter().map(|v| v * v).sum::<f32>().sqrt() > 4.
            {
                return Err("invalid authored bed geometry".into());
            }
        }
        Ok(())
    }
}
impl Engine {
    pub fn configure_authored_processing(&mut self, settings: Processing) -> Result<(), String> {
        settings.validate()?;
        self.authored = Processing::default();
        if !settings.enabled {
            self.source_occlusion_enabled = true;
            self.refresh_stereo_bass();
            return Ok(());
        }
        self.set_spatial_enhancement(settings.spatial_presentation)?;
        self.set_binaural_enabled(true);
        self.authored = settings;
        self.program_enabled = !self.authored.programme_handled;
        self.program_metadata_gain = 10f32.powf(self.authored.programme_gain_db / 20.);
        self.program_events.clear();
        self.refresh_programme_output_makeup(true);
        self.set_program_target(self.program_metadata_gain, true);
        if self.authored.source_spatial_handled {
            let flat = dsp::Biquad::highshelf(
                self.output_sample_rate,
                4000.,
                std::f32::consts::FRAC_1_SQRT_2,
                0.,
            )?;
            self.output_clarity = [flat; 2];
            self.auxiliary_balance
                .set_enabled(false, true, self.block_offset);
        }
        self.refresh_stereo_bass();
        let positions = self
            .authored
            .bed_positions
            .iter()
            .map(|v| (v.id.clone(), (v.position, v.gain_db)))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut reroute = vec![];
        for (id, source) in &mut self.sources {
            source.authored_bed_position = positions.get(id).map(|v| v.0);
            if let Some((_, db)) = positions.get(id) {
                source.gain = cinema::db(*db);
                source.target_gain = source.gain;
                source.ramp_remaining = 0;
            }
            source.muted = self.authored.muted_sources.contains(id);
            if source.kind == SourceKind::Bed {
                reroute.push(id.clone());
            }
        }
        for id in reroute {
            self.route_source_now(&id, 0)?;
        }
        Ok(())
    }
    pub fn authored_processing_active(&self) -> bool {
        self.authored.enabled
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn baked_file_policy_does_not_apply_user_balance_or_makeup_again() {
        let mut e = Engine::new(48000, 2);
        e.set_program_codec("alac".into());
        e.configure_authored_processing(Processing {
            enabled: true,
            version: 1,
            programme_handled: true,
            source_spatial_handled: true,
            spatial_presentation: true,
            stereo_source: true,
            ..Default::default()
        })
        .unwrap();
        assert!(!e.program_enabled);
        assert_eq!(e.program_gain, 1.);
        assert_eq!(e.programme_output_makeup.target_db(), 0.);
        e.set_spatial_enhancement(false).unwrap();
        assert!(e.spatial_enhancement_enabled);
        e.set_program_target(0.2, true);
        assert_eq!(e.program_gain, 1.);
        assert_eq!(e.programme_output_makeup.target_db(), 0.);
        e.reset_session(0);
        assert!(!e.authored.enabled);
        e.set_spatial_enhancement(false).unwrap();
        assert!(!e.spatial_enhancement_enabled);
    }
    #[test]
    fn metadata_only_programme_gain_uses_file_values_without_new_analysis() {
        let mut e = Engine::new(48000, 2);
        e.configure_authored_processing(Processing {
            enabled: true,
            version: 1,
            programme_handled: false,
            source_spatial_handled: true,
            spatial_presentation: false,
            programme_gain_db: -7.,
            makeup_db: 1.25,
            ..Default::default()
        })
        .unwrap();
        assert!((20. * e.program_gain.log10() + 7.).abs() < 1e-5);
        assert_eq!(e.programme_output_makeup.target_db(), 1.25);
        e.set_program_target(0.01, true);
        assert!((20. * e.program_gain.log10() + 7.).abs() < 1e-5);
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scene {
    pub version: u32,
    pub layout: String,
    pub render: serde_json::Value,
    pub resources: Resources,
    pub processing: Processing,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Resources {
    pub root: std::path::PathBuf,
    pub hrtf: std::path::PathBuf,
    pub hrtf_wet: f32,
    pub room: Option<std::path::PathBuf>,
    pub headphone: Option<Headphone>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Headphone {
    pub left: std::path::PathBuf,
    pub right: std::path::PathBuf,
    pub preamp: f32,
}
fn bounded(value: &serde_json::Value, default: f32, min: f32, max: f32) -> Result<f32, String> {
    let v = if value.is_null() {
        default
    } else {
        value
            .as_f64()
            .ok_or("Unsupported authored numeric parameter")? as f32
    };
    if !v.is_finite() || v < min || v > max {
        return Err("Unsupported authored render parameter".into());
    }
    Ok(v)
}
fn string_list(value: &serde_json::Value) -> Result<Vec<String>, String> {
    if value.is_null() {
        return Ok(vec![]);
    }
    let values = value.as_array().ok_or("Invalid authored routing list")?;
    if values.len() > 128 {
        return Err("Authored routing list too large".into());
    }
    values
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|v| v.len() < 128)
                .map(String::from)
                .ok_or("Invalid authored routing identity".into())
        })
        .collect()
}
fn authored_room(
    profile: cinema::RoomProfile,
    layout: vbap::LayoutId,
) -> Result<cinema::RoomProfile, String> {
    if profile.layout == layout.as_str() {
        return Ok(profile);
    }
    let mut next = profile.clone();
    next.layout = layout.as_str().into();
    let direction = |a: f32, e: f32| static_bed_position(a, e);
    let mut speakers = vec![];
    for target in vbap::speakers(layout) {
        if let Some(original) = profile.speakers.iter().find(|s| {
            s.name == target.name
                && (s.azimuth - target.azimuth).abs() < 1.
                && (s.elevation - target.elevation).abs() < 1.
        }) {
            speakers.push(original.clone());
            continue;
        }
        let p = direction(target.azimuth, target.elevation);
        let mut candidates = profile
            .speakers
            .iter()
            .filter(|s| (s.elevation >= 30.) == (target.elevation >= 30.))
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            candidates = profile.speakers.iter().collect();
        }
        candidates.sort_by(|a, b| {
            let score = |s: &&cinema::RoomSpeaker| {
                let v = direction(s.azimuth, s.elevation);
                1. - v
                    .iter()
                    .zip(p)
                    .map(|(a, b)| a * b)
                    .sum::<f32>()
                    .clamp(-1., 1.)
            };
            score(a).total_cmp(&score(b))
        });
        let left = *candidates
            .first()
            .ok_or("Authored room has no usable directions")?;
        let right = *candidates.get(1).unwrap_or(&left);
        let angle = |s: &cinema::RoomSpeaker| {
            let v = direction(s.azimuth, s.elevation);
            v.iter()
                .zip(p)
                .map(|(a, b)| a * b)
                .sum::<f32>()
                .clamp(-1., 1.)
                .acos()
                .max(1e-6)
        };
        let a = angle(left);
        let b = angle(right);
        let weight = b / (a + b);
        let combine = |x: &[f32], y: &[f32]| {
            (0..x.len().max(y.len()))
                .map(|i| {
                    x.get(i).copied().unwrap_or(0.) * weight
                        + y.get(i).copied().unwrap_or(0.) * (1. - weight)
                })
                .collect::<Vec<_>>()
        };
        let (direct_left, direct_right, room_left, room_right) = (
            combine(&left.direct_left, &right.direct_left),
            combine(&left.direct_right, &right.direct_right),
            combine(&left.room_left, &right.room_left),
            combine(&left.room_right, &right.room_right),
        );
        let peak = |v: &[f32]| {
            v.iter()
                .enumerate()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .map_or(0, |v| v.0)
        };
        let onset_sample = peak(&direct_left).min(peak(&direct_right));
        speakers.push(cinema::RoomSpeaker {
            name: target.name.into(),
            azimuth: target.azimuth,
            elevation: target.elevation,
            onset_sample,
            direct_left,
            direct_right,
            room_left,
            room_right,
        });
    }
    next.speakers = speakers;
    if let Some(simulation) = next
        .simulation
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
    {
        simulation.insert("sdarLayoutAdaptation".into(),serde_json::json!({"source":profile.layout,"target":next.layout,"method":"preserve-common-speakers-interpolate-extra-directions-v1"}));
    }
    next.validate()?;
    Ok(next)
}
impl Engine {
    pub fn configure_authored_scene(&mut self, scene: Scene) -> Result<(), String> {
        if scene.version != 1
            || scene.render["schema"] != "sda.render-metadata"
            || scene.render["version"] != 1
        {
            return Err("unsupported authored scene version".into());
        }
        if self.output_active && !self.paused {
            return Err("authored scene replacement requires paused transport".into());
        }
        scene.processing.validate()?;
        let layout =
            vbap::LayoutId::parse(&scene.layout).ok_or("unsupported authored output layout")?;
        let root = scene
            .resources
            .root
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let check = |path: &std::path::Path| -> Result<(), String> {
            let resolved = path.canonicalize().map_err(|e| e.to_string())?;
            if resolved == root || !resolved.starts_with(&root) {
                return Err("authored resource outside its source package".into());
            }
            Ok(())
        };
        check(&scene.resources.hrtf)?;
        let candidate = hrtf::NativeHrtfSet::load_calibrated(&scene.resources.hrtf)?;
        let snapshot = &scene.render["playbackSnapshot"];
        if snapshot["schema"] != "sda.playback-snapshot" || snapshot["version"] != 1 {
            return Err("authored playback parameters missing".into());
        }
        let settings: cinema::Settings =
            serde_json::from_value(snapshot["cinema"]["settings"].clone())
                .map_err(|e| format!("authored room settings: {e}"))?;
        settings.validate()?;
        let room = scene
            .resources
            .room
            .as_ref()
            .map(|path| {
                check(path)?;
                let profile =
                    cinema::RoomProfile::load(path.to_str().ok_or("invalid authored room path")?)?;
                authored_room(profile, layout).map(std::sync::Arc::new)
            })
            .transpose()?;
        if settings.enabled && snapshot["cinema"]["profileId"].as_str().is_some() && room.is_none()
        {
            return Err("authored room response missing".into());
        }
        let interaction = &snapshot["sourceOcclusion"];
        let source_occlusion = if interaction.is_null() { true } else {
            if interaction["algorithm"] != "source-bearing-8deg-v1" {
                return Err("unsupported source occlusion algorithm".into());
            }
            interaction["enabled"].as_bool().ok_or("source occlusion enabled must be boolean")?
        };
        let near: near_field::Settings = serde_json::from_value(snapshot["nearField"].clone())
            .map_err(|e| format!("authored near-field settings: {e}"))?;
        if !near.valid() {
            return Err("unsupported authored near-field values".into());
        }
        let extent: source_extent::Settings =
            serde_json::from_value(snapshot["sourceExtent"].clone())
                .map_err(|e| format!("authored source extent: {e}"))?;
        if !extent.valid() {
            return Err("unsupported authored source extent values".into());
        }
        let bands = &snapshot["equalizer"]["bands"];
        if !matches!(
            snapshot["equalizer"]["lowFrequencyDiagnostic"].as_str(),
            Some("reference" | "low-cut")
        ) {
            return Err("unsupported authored EQ diagnostic mode".into());
        }
        let eq = StereoEq::new(
            self.output_sample_rate,
            [
                bounded(&bands["low"], 0., -12., 12.)?,
                bounded(&bands["mid"], 0., -12., 12.)?,
                bounded(&bands["high"], 0., -12., 12.)?,
            ],
            snapshot["equalizer"]["lowFrequencyDiagnostic"] == "low-cut",
        )?;
        let headphone = if let Some(h) = &scene.resources.headphone {
            check(&h.left)?;
            check(&h.right)?;
            let taps = |p: &std::path::Path| -> Result<Vec<f32>, String> {
                let b = std::fs::read(p).map_err(|e| e.to_string())?;
                if b.len() < 8 || b.len() > 32768 * 4 || b.len() % 4 != 0 {
                    return Err("invalid authored headphone filter".into());
                }
                Ok(b.chunks_exact(4)
                    .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
                    .collect())
            };
            headphone::HeadphoneCompensation::new(&taps(&h.left)?, &taps(&h.right)?, h.preamp)?
        } else {
            headphone::HeadphoneCompensation::bypass()?
        };
        let comparison = bounded(&snapshot["monitoring"]["comparisonGainDb"], 0., -40., 0.)?;
        let master = bounded(&snapshot["native"]["masterPreampDb"], 0., -12., 12.)?;
        let mutes = string_list(&snapshot["routing"]["mutedSpeakers"])?;
        let focus = string_list(&snapshot["routing"]["activeFocus"])?;
        let epoch = self.render_epoch;
        let monitor = (self.binaural_enabled, self.binaural_mix, self.plain_warmup);
        self.authored = Processing::default();
        self.layout = layout;
        self.vbap = vbap::VbapSolver::with_layout(layout);
        self.configure_room_with_hrtf(settings, room, candidate, scene.resources.hrtf_wet)?;
        self.set_direct_objects(
            snapshot["hrtf"]["directObjectHrtf"]
                .as_bool()
                .unwrap_or(true),
        )?;
        self.set_directional_hrtf(
            snapshot["native"]["directionalHrtf"]
                .as_bool()
                .unwrap_or(true),
        );
        self.configure_near_field(near)?;
        self.source_extent = extent;
        self.source_occlusion_enabled = source_occlusion;
        self.binaural_eq = eq;
        self.headphone = headphone;
        self.master_preamp
            .set(master, self.output_sample_rate, true)?;
        self.comparison_gain = cinema::db(comparison);
        self.comparison_target = self.comparison_gain;
        self.set_speaker_monitor(mutes, focus);
        self.stereo_mode = StereoMode::Room;
        self.configure_authored_processing(scene.processing)?;
        self.set_binaural_enabled(monitor.0);
        self.binaural_mix = monitor.1;
        self.plain_warmup = monitor.2;
        self.render_epoch = epoch;
        Ok(())
    }
}

#[cfg(test)]
mod scene_tests {
    use super::*;
    use serde_json::json;
    struct Fixture {
        root: std::path::PathBuf,
        scene: Scene,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let base = std::env::temp_dir().canonicalize().unwrap();
            let root = self.root.canonicalize().unwrap();
            assert_eq!(root.parent(), Some(base.as_path()));
            assert!(root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("sda-authored-scene-"));
            let _ = std::fs::remove_dir_all(root);
        }
    }
    fn fixture() -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "sda-authored-scene-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let original = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../web/public/hrtf");
        let manifest = std::fs::read(original.join("hrtf-set.json")).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
        for row in value["positions"].as_array().unwrap() {
            for key in ["dry", "wet"] {
                let name = row[key].as_str().unwrap();
                std::fs::copy(original.join(name), root.join(name)).unwrap();
            }
        }
        if let Some(name) = value["triangleSupportFile"].as_str() {
            std::fs::copy(original.join(name), root.join(name)).unwrap();
        }
        std::fs::write(root.join("hrtf-set.json"), manifest).unwrap();
        let impulse = |tail: bool| {
            let mut v = vec![0f32; 512];
            v[32] = 0.4;
            if tail {
                v[300] = 0.05;
            }
            v
        };
        let speakers=vbap::speakers(vbap::LayoutId::Dolby7_1_4).iter().map(|s|json!({"name":s.name,"azimuth":s.azimuth,"elevation":s.elevation,"onsetSample":32,"directLeft":impulse(false),"directRight":impulse(false),"roomLeft":impulse(true),"roomRight":impulse(true)})).collect::<Vec<_>>();
        let profile = json!({"version":1,"name":"Authored transfer test","source":"fixture","license":"test","measurement":"simulated","simulation":{"sourceModel":"ideal-omnidirectional"},"sampleRate":48000,"layout":"7.1.4","speakers":speakers});
        std::fs::write(
            root.join("room.json"),
            serde_json::to_vec(&profile).unwrap(),
        )
        .unwrap();
        for name in ["left.f32", "right.f32"] {
            std::fs::write(
                root.join(name),
                [1f32.to_le_bytes(), 0f32.to_le_bytes()].concat(),
            )
            .unwrap();
        }
        let scene:Scene=serde_json::from_value(json!({"version":1,"layout":"7.1.4","render":{"schema":"sda.render-metadata","version":1,"playbackSnapshot":{"schema":"sda.playback-snapshot","version":1,"hrtf":{"directObjectHrtf":true},"native":{"directionalHrtf":true,"masterPreampDb":-2.5},"cinema":{"profileId":"authored-fixture-room","settings":{"enabled":true,"directDb":-2,"earlyDb":-8,"lateDb":-4,"earlyMs":40}},"nearField":{"enabled":false,"metresPerUnit":1.2},"sourceExtent":{"enabled":true,"width":0.2,"diffusion":0.3},"equalizer":{"bands":{"low":1.25,"mid":-0.5,"high":0.75},"lowFrequencyDiagnostic":"reference"},"monitoring":{"comparisonGainDb":-3},"routing":{"mutedSpeakers":[],"activeFocus":[]}}},"resources":{"root":root,"hrtf":root.join("hrtf-set.json"),"hrtfWet":0.04,"room":root.join("room.json"),"headphone":{"left":root.join("left.f32"),"right":root.join("right.f32"),"preamp":0.8}},"processing":{"enabled":true,"version":1,"programmeHandled":true,"sourceSpatialHandled":true,"spatialPresentation":true,"stereoSource":true,"programmeGainDb":-12,"makeupDb":2.5,"bedPositions":[{"id":"bed:0","position":static_bed_position(30.,0.),"gainDb":-1.5}],"mutedSources":[]}})).unwrap();
        Fixture { root, scene }
    }
    fn rendered(e: &mut Engine) -> Vec<f32> {
        let mut source = Source {
            kind: SourceKind::Bed,
            bed_label: Some("FrontLeft".into()),
            gain: 1.,
            target_gain: 1.,
            availability: 1.,
            availability_target: 1.,
            ..Default::default()
        };
        source.samples.write(
            0,
            0,
            &(0..8192)
                .map(|i| 0.01 * (i as f32 * 0.13).sin())
                .collect::<Vec<_>>(),
        );
        e.sources.insert("bed:0".into(), source);
        let p = e.authored.clone();
        e.configure_authored_processing(p).unwrap();
        e.output_active = true;
        e.paused = false;
        let mut output = vec![0.; 16384];
        e.render_into(&mut output, 2);
        output
    }
    #[test]
    fn authored_source_occlusion_is_portable_and_monitor_bypass_remains_available() {
        let mut f=fixture();
        f.scene.render["playbackSnapshot"]["sourceOcclusion"]=json!({"enabled":false,"algorithm":"source-bearing-8deg-v1"});
        let mut e=Engine::new(48000,2);e.configure_authored_scene(f.scene.clone()).unwrap();
        assert!(!e.source_occlusion_enabled);
        let recipe=e.authored.clone();let clock=e.sample_pos;
        let fifo=stereo_fifo::StereoFifo::new(4096);let telemetry=RuntimeTelemetry::default();
        protocol::apply_render_command(&mut e,render_command::RenderCommand::Command(Command::SetBinauralEnabled{enabled:false}),&fifo,&telemetry);
        assert!(!e.binaural_enabled,"authored monitor Off must be accepted");assert_eq!(e.sample_pos,clock);
        assert_eq!(e.authored.enabled,recipe.enabled);
        f.scene.render["playbackSnapshot"]["sourceOcclusion"]["enabled"]=json!(true);
        e.configure_authored_scene(f.scene.clone()).unwrap();assert!(e.source_occlusion_enabled);
        assert!(!e.binaural_enabled,"scene/layout replacement must retain head Off");
        assert_eq!(e.binaural_mix,0.0);assert_eq!(e.plain_warmup,0);
        f.scene.render["playbackSnapshot"]["sourceOcclusion"]["algorithm"]=json!("unknown");
        assert!(e.configure_authored_scene(f.scene.clone()).is_err());
    }

    #[test]
    fn full_authored_scene_replaces_user_preset_and_retains_room_eq_and_gain() {
        let fixture = fixture();
        let mut a = Engine::new(48000, 2);
        let mut b = Engine::new(48000, 2);
        a.set_spatial_enhancement(true).unwrap();
        a.program_enabled = true;
        a.set_program_target(0.1, true);
        b.set_spatial_enhancement(false).unwrap();
        b.program_enabled = false;
        a.configure_authored_scene(fixture.scene.clone()).unwrap();
        b.configure_authored_scene(fixture.scene.clone()).unwrap();
        assert!(a.cinema.enabled);
        assert_eq!(a.cinema.early_db, -8.);
        assert_eq!(a.cinema.late_db, -4.);
        assert_eq!(
            a.room_profile.as_ref().unwrap().name,
            "Authored transfer test"
        );
        assert_eq!(a.master_preamp.target_db(), -2.5);
        assert!((a.comparison_gain - cinema::db(-3.)).abs() < 1e-6);
        assert!(a.source_extent.enabled);
        assert!(!a.program_enabled);
        assert_eq!(a.program_gain, 1.);
        assert_eq!(a.programme_output_makeup.target_db(), 0.);
        let output = rendered(&mut a);
        assert!(output.iter().any(|v| v.abs() > 1e-6));
        assert_eq!(
            output,
            rendered(&mut b),
            "saved player balance cannot alter authored PCM"
        );
        let mut dry = fixture.scene.clone();
        dry.render["playbackSnapshot"]["cinema"]["settings"]["enabled"] = json!(false);
        let mut c = Engine::new(48000, 2);
        c.configure_authored_scene(dry).unwrap();
        assert!(
            output
                .iter()
                .zip(rendered(&mut c))
                .any(|(x, y)| (*x - y).abs() > 1e-6),
            "embedded room must audibly affect the shared output"
        );
    }
    #[test]
    fn exported_upmix_matches_live_renderer_across_output_layouts() {
        let fixture = fixture();
        let labels = [
            "FrontLeft",
            "FrontRight",
            "Center",
            "LFE",
            "SurroundLeft",
            "SurroundRight",
            "RearLeft",
            "RearRight",
            "TopFrontLeft",
            "TopFrontRight",
            "TopRearLeft",
            "TopRearRight",
        ];
        let order = [0usize, 1, 2, 3, 6, 7, 4, 5, 8, 9, 10, 11];
        let frames = 16384;
        let mut raw = vec![vec![0f32; frames]; 12];
        let trim = 10f32.powf(-2. / 20.);
        for i in 0..frames {
            let wave = |hz: f32| (std::f32::consts::TAU * hz * i as f32 / 48000.).sin();
            let l = 0.02 * wave(997.) + 0.006 * wave(80.) + 0.005 * wave(530.);
            let r = 0.015 * wave(701.) + 0.006 * wave(80.) - 0.005 * wave(530.);
            let side = (l - r) * 0.5;
            let values = [
                l * 0.8,
                r * 0.8,
                (l + r) * 0.25,
                (l + r) * 0.125,
                side * 0.34,
                -side * 0.34,
                side * 0.2,
                -side * 0.2,
                side * 0.14,
                -side * 0.14,
                side * 0.1,
                -side * 0.1,
            ];
            for ch in 0..12 {
                raw[ch][i] = values[ch] * trim;
            }
        }
        for volume in [false, true] {
            let programme_db = if volume { -6. } else { 0. };
            let makeup_db = 0.0; // Ordinary stereo upmix has no extra programme makeup.
            for spatial in [false, true] {
                let mut processor =
                    crate::bed_export::BedProcessor::new(programme_db, makeup_db, spatial).unwrap();
                let mut baked = vec![vec![0.; frames]; 12];
                for i in 0..frames {
                    let out = processor
                        .process(std::array::from_fn(|ch| raw[ch][i]))
                        .unwrap();
                    for ch in 0..12 {
                        baked[ch][i] = out[ch];
                    }
                }
                for layout in ["5.1", "7.1.2", "7.1.4", "9.1.6", "11.1.8", "22.2"] {
                    let mut scene = fixture.scene.clone();
                    scene.layout = layout.into();
                    scene.processing.spatial_presentation = spatial;
                    scene.processing.programme_gain_db = programme_db;
                    scene.processing.makeup_db = makeup_db;
                    scene.processing.bed_positions.clear();
                    let mut live = Engine::new(48000, 2);
                    live.configure_authored_scene(scene.clone()).unwrap();
                    live.authored = Processing::default();
                    live.set_program_codec("alac".into());
                    live.program_enabled = volume;
                    live.set_spatial_enhancement(spatial).unwrap();
                    live.set_program_target(cinema::db(programme_db), true);
                    let mut authored = Engine::new(48000, 2);
                    authored.configure_authored_scene(scene).unwrap();
                    for (e, is_file) in [(&mut live, false), (&mut authored, true)] {
                        for ch in 0..12 {
                            let original = if is_file { order[ch] } else { ch };
                            let label = labels[original];
                            let mut source = Source {
                                kind: SourceKind::Bed,
                                bed_label: Some(label.into()),
                                gain: 1.,
                                target_gain: 1.,
                                availability: 1.,
                                availability_target: 1.,
                                ..Default::default()
                            };
                            source.samples.write(
                                0,
                                0,
                                if is_file {
                                    &baked[original]
                                } else {
                                    &raw[original]
                                },
                            );
                            Engine::set_source_route(&mut source, bed_route(label, &e.vbap), 0);
                            e.sources.insert(format!("bed:{ch}"), source);
                        }
                        e.output_active = true;
                        e.paused = false;
                    }
                    let mut expected = vec![0.; frames * 2];
                    let mut actual = expected.clone();
                    live.render_into(&mut expected, 2);
                    authored.render_into(&mut actual, 2);
                    let maximum = expected
                        .iter()
                        .zip(&actual)
                        .map(|(a, b)| (a - b).abs())
                        .fold(0f32, f32::max);
                    assert!(
                    maximum < 2e-5,
                    "layout={layout} spatial={spatial} volume={volume} max difference={maximum}"
                );
                    assert!(actual.iter().any(|v| v.abs() > 1e-6));
                }
            }
        }
    }
    #[test]
    fn intrinsic_hrtf_room_can_be_authored_without_an_external_profile() {
        let fixture = fixture();
        let mut scene = fixture.scene.clone();
        scene.resources.room = None;
        scene.render["playbackSnapshot"]["cinema"]["profileId"] = serde_json::Value::Null;
        scene.render["playbackSnapshot"]["cinema"]["settings"]["reflectionMode"] = json!("early");
        let mut engine = Engine::new(48000, 2);
        engine.configure_authored_scene(scene).unwrap();
        assert!(engine.cinema.enabled && engine.room_profile.is_none());
        assert!(matches!(
            engine.cinema.reflection_mode,
            cinema::ReflectionMode::Early
        ));
        assert!(engine.authored.enabled);
        assert_eq!(engine.comparison_gain, cinema::db(-3.));
    }
    #[test]
    fn authored_room_adapts_to_more_speakers_and_invalid_recipe_fails_before_replacement() {
        let fixture = fixture();
        let original = cinema::RoomProfile::load(
            fixture
                .scene
                .resources
                .room
                .as_ref()
                .unwrap()
                .to_str()
                .unwrap(),
        )
        .unwrap();
        let adapted = authored_room(original.clone(), vbap::LayoutId::Dolby9_1_6).unwrap();
        assert_eq!(
            adapted.speakers.len(),
            vbap::speakers(vbap::LayoutId::Dolby9_1_6).len()
        );
        let old = original
            .speakers
            .iter()
            .find(|s| s.name == "FrontLeft")
            .unwrap();
        let new = adapted
            .speakers
            .iter()
            .find(|s| s.name == "FrontLeft")
            .unwrap();
        assert_eq!(old.room_left, new.room_left);
        assert_eq!(old.direct_right, new.direct_right);
        let mut e = Engine::new(48000, 2);
        e.configure_authored_scene(fixture.scene.clone()).unwrap();
        let mut invalid = fixture.scene.clone();
        invalid.resources.room = None;
        assert!(e
            .configure_authored_scene(invalid)
            .unwrap_err()
            .contains("room response missing"));
        assert!(e.cinema.enabled && e.authored.enabled);
        let epoch = e.render_epoch;
        let mut next = fixture.scene.clone();
        next.layout = "9.1.6".into();
        e.configure_authored_scene(next).unwrap();
        assert_eq!(e.layout, vbap::LayoutId::Dolby9_1_6);
        assert_eq!(e.render_epoch, epoch, "layout adaptation is not a seek");
    }
}
