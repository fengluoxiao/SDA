//! Integration checks and optional local replay of the user-approved presentation.
use super::*;
use std::path::{Path, PathBuf};
fn manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json")
}
#[test]
fn profile_v3_applies_once_and_preserves_front_measurements_and_fallback() {
    let original = hrtf::NativeHrtfSet::load_without_shared_clarity(&manifest()).unwrap();
    let prepared = hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap();
    let reloaded = hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap();
    assert!(!original.shared_clarity_active());
    assert!(prepared.shared_clarity_active());
    assert!(prepared.preparation_snapshot().shared_clarity_active());
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(manifest()).unwrap()).unwrap();
    let mut changed = 0;
    for p in document["positions"].as_array().unwrap() {
        let a = p["azimuth"].as_f64().unwrap();
        let el = p["elevation"].as_f64().unwrap();
        let raw = original.mixed_direction(a, el, 0.).unwrap();
        let actual = prepared.mixed_direction(a, el, 0.).unwrap();
        let repeat = reloaded.mixed_direction(a, el, 0.).unwrap();
        assert_eq!(actual, repeat, "profile reload accumulated the EQ");
        assert_eq!(actual.0.len(), 640);
        assert_eq!(actual.1.len(), 640);
        for (before, after) in [(&raw.0, &actual.0), (&raw.1, &actual.1)] {
            if a.abs() <= 45. {
                assert_eq!(before.as_slice(), &after[..512]);
                assert!(after[512..].iter().all(|v| *v == 0.));
            } else if before.iter().zip(after).any(|(a, b)| (a - b).abs() > 1e-6) {
                changed += 1;
            }
            assert!(after.iter().all(|v| v.is_finite()));
        }
    }
    assert!(changed > 20, "the new response policy did not apply");
    let fallback = hrtf::NativeHrtfSet::load_calibrated(
        &manifest()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("hrtf/hrtf-set.json"),
    )
    .unwrap();
    assert!(fallback.shared_clarity_active());
}
#[test]
fn profile_v3_rejects_missing_unknown_or_cross_family_clarity_policy() {
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(manifest()).unwrap()).unwrap();
    let root = std::env::temp_dir().join(format!("sda-clarity-policy-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("hrtf-set.json");
    for (version, clarity) in [(3, None), (3, Some(2)), (2, Some(1)), (1, Some(1))] {
        let mut bad = original.clone();
        bad["measuredResponseVersion"] = version.into();
        if let Some(value) = clarity {
            bad["sharedClarityVersion"] = value.into();
        } else {
            bad.as_object_mut().unwrap().remove("sharedClarityVersion");
        }
        std::fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
        let error = hrtf::NativeHrtfSet::load_calibrated(&path).err().unwrap();
        assert!(
            error.contains("Measured response profile"),
            "wrong rejection: {error}"
        );
    }
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(root).unwrap();
}
#[test]
fn profile_v3_live_reload_keeps_gain_override() {
    let mut e = Engine::new(48000, 2);
    e.configure_hrtf_preset(
        hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap(),
        0.,
        true,
        true,
    )
    .unwrap();
    assert!(e.active_hrtf_set.as_ref().unwrap().shared_clarity_active());
    assert_eq!(e.spatial_layer.rear_db, 0.);
    e.set_spatial_enhancement(false).unwrap();
    e.reset_session(0);
    e.configure_hrtf_preset(
        hrtf::NativeHrtfSet::load_calibrated(&manifest()).unwrap(),
        0.,
        true,
        true,
    )
    .unwrap();
    assert_eq!(e.spatial_layer.rear_db, 0.);
    assert_eq!(e.spatial_layer.db, 0.);
    assert!(
        e.active_hrtf_set.as_ref().unwrap().shared_clarity_active(),
        "clarity belongs to the selected HRTF family"
    );
    // The current Off path is exercised by the existing compatible-bed and
    // plain_downmix engine tests; the low-level enhancement controls are separate.
}

#[test]
#[ignore = "requires SDA_APPROVED_SCENE_DIR, optional codec/seconds; local user PCM only"]
fn replay_approved_clarity_scene() {
    replay_scene(None);
}

#[test]
#[ignore = "requires SDA_ARCHIVED_AUDITION_CONFIG and local source fixture; not a production preset"]
fn replay_archived_presentation() {
    let path = std::env::var("SDA_ARCHIVED_AUDITION_CONFIG").unwrap();
    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    replay_scene(Some(config));
}

fn replay_scene(audition: Option<serde_json::Value>) {
    use std::io::{BufRead, Read, Write};
    let dir = PathBuf::from(std::env::var("SDA_APPROVED_SCENE_DIR").unwrap());
    let codec = std::env::var("SDA_APPROVED_SCENE_CODEC").unwrap_or_else(|_| "eac3".into());
    assert!(matches!(codec.as_str(), "eac3" | "mpegh" | "adm" | "av3a"));
    let seconds: u64 = std::env::var("SDA_APPROVED_SCENE_SECONDS")
        .unwrap()
        .parse()
        .unwrap();
    let tag = std::env::var("SDA_APPROVED_SCENE_TAG").unwrap_or_else(|_| "integrated".into());
    let mut e = Engine::new(48000, 2);
    if codec == "mpegh" {
        e.set_layout(vbap::LayoutId::Sony360Ra13).unwrap();
    }
    let profile = audition
        .as_ref()
        .and_then(|c| c["manifest"].as_str())
        .map(PathBuf::from)
        .unwrap_or_else(manifest);
    let without_clarity = audition
        .as_ref()
        .is_some_and(|c| c["withoutClarity"].as_bool() == Some(true));
    let mut set = if without_clarity {
        hrtf::NativeHrtfSet::load_without_shared_clarity(&profile)
    } else {
        hrtf::NativeHrtfSet::load_calibrated(&profile)
    }
    .unwrap();
    if let Some(c) = &audition {
        if let Some(enabled) = c["frontIsolation"].as_bool() {
            set = set.diagnostic_front_ear_isolation(enabled);
        }
        if c["withoutDepth"].as_bool() == Some(true) {
            set = set.diagnostic_without_shared_depth();
        }
        if let Some(gain) = c["cueGain"].as_f64() {
            if gain == 0. {
                set = set.diagnostic_without_spatial_residual();
            } else {
                set.set_spatial_cue_gain(gain as f32).unwrap();
            }
        }
    }
    e.replace_hrtf(set, 0.).unwrap();
    let integrated_base = audition
        .as_ref()
        .is_none_or(|c| c["useIntegratedBase"].as_bool() == Some(true));
    if integrated_base {
        assert!(
            e.active_hrtf_set.as_ref().unwrap().shared_clarity_active()
                || audition
                    .as_ref()
                    .is_some_and(|c| c["withoutClarity"].as_bool() == Some(true))
        );
        assert_eq!(e.spatial_layer.geometry, spatial_layer::Geometry::Front45);
        assert_eq!(e.spatial_preamp.target_db(), 2.);
        assert_eq!(e.spatial_layer.db, 4.);
        assert_eq!(e.spatial_layer.rear_db, 0.);
        e.set_master_preamp_db(-12.).unwrap();
        e.set_output_volume(0.5, true);
        if let Some(c) = &audition {
            if let Some(db) = c["auxDb"].as_f64() {
                e.set_spatial_layer_gain_db(db as f32).unwrap();
            }
            if let Some(db) = c["rearDb"].as_f64() {
                assert!(db.is_finite() && (0.0..=3.0).contains(&db));
                e.spatial_layer.rear_db = db as f32;
            }
            // Freeze earlier A/C/D/E audition definitions as production defaults evolve.
            let db = c["highShelfDb"].as_f64().unwrap_or(0.);
            assert!(db.is_finite() && (0.0..=1.5).contains(&db));
            e.output_clarity =
                [dsp::Biquad::highshelf(48000, 4000., std::f32::consts::FRAC_1_SQRT_2, db as f32)
                    .unwrap(); 2];
        }
    } else if let Some(c) = &audition {
        // Keep archived audition definitions independent of new production defaults.
        let enhancement = c["enhancement"].as_bool().unwrap_or(true);
        e.set_spatial_enhancement(false).unwrap();
        e.spatial_layer.geometry = if c["renderer"].as_str() == Some("historical-aux") {
            spatial_layer::Geometry::Front45
        } else {
            spatial_layer::Geometry::ProtectedSides
        };
        e.output_clarity = [dsp::Biquad::highshelf(
            48000,
            4000.,
            std::f32::consts::FRAC_1_SQRT_2,
            if enhancement { 1.5 } else { 0. },
        )
        .unwrap(); 2];
        e.set_spatial_layer_gain_db(c["auxDb"].as_f64().unwrap_or(if enhancement {
            1.5
        } else {
            0.
        }) as f32)
            .unwrap();
        e.spatial_layer.rear_db = c["rearDb"]
            .as_f64()
            .unwrap_or(if enhancement { 3. } else { 0. }) as f32;
        assert!(
            e.spatial_layer.rear_db.is_finite() && (0.0..=3.0).contains(&e.spatial_layer.rear_db)
        );
        e.spatial_layer.main_restore_db = c["mainRestoreDb"].as_f64().unwrap_or(0.) as f32;
        assert!(
            e.spatial_layer.main_restore_db.is_finite()
                && (0.0..=6.0).contains(&e.spatial_layer.main_restore_db)
        );
        e.set_master_preamp_db(c["masterDb"].as_f64().unwrap_or(0.) as f32 - 12.)
            .unwrap();
        e.set_output_volume(0.5, true);
        let trim = c["postGuardDb"].as_f64().unwrap_or(0.) as f32;
        assert!(trim.is_finite() && (-6.0..=0.0).contains(&trim));
        e.final_output_trim.set(trim, 48000, true).unwrap();
        e.set_spatial_balance(c["foregroundAttenuation"].as_bool().unwrap_or(false));
    }
    if let Some(c) = &audition {
        // Archived comparisons remain frozen as the default evolves.
        let enabled = c["frontCommon"].as_bool().unwrap_or(false);
        e.front_common.enabled = enabled;
        e.front_common.grouped = c["groupedFrontCommon"].as_bool().unwrap_or(false);
        assert!(!e.front_common.grouped || enabled);
        let db = c["frontCommonPresenceDb"].as_f64().unwrap_or(0.) as f32;
        assert!(enabled || db == 0.);
        e.front_common.set_presence(48000, db).unwrap();
    } else {
        assert!(e.front_common.enabled && e.front_common.grouped);
        assert_eq!(e.front_common.presence_db(), 3.);
    }
    let unity = std::env::var("SDA_APPROVED_SCENE_UNITY_OUTPUT").as_deref() == Ok("1");
    if unity { e.set_master_preamp_db(0.).unwrap(); e.set_output_volume(1., true); }
    let programme_db=std::env::var("SDA_APPROVED_SCENE_PROGRAMME_GAIN_DB").ok()
        .map(|v|v.parse::<f32>().unwrap());
    if let Some(db)=programme_db {
        assert!(db.is_finite() && (-60.0..=0.0).contains(&db));
        e.program_enabled=true;e.program_metadata_gain=10f32.powf(db/20.);
        e.set_program_target(e.program_metadata_gain,true);
    }
    if std::env::var("SDA_APPROVED_SCENE_SPATIAL_STATE").as_deref()==Ok("off") {
        e.set_spatial_enhancement(false).unwrap();
    }
    if std::env::var("SDA_APPROVED_SCENE_AUXILIARY_CONTROL").as_deref()==Ok("off") {
        e.auxiliary_balance=auxiliary_balance::AuxiliaryBalance::with_policy(48000,
            auxiliary_balance::Policy {maximum_cut_db:0.,..Default::default()});
    }
    let mut gain_trace=(std::env::var("SDA_APPROVED_SCENE_GAIN_TRACE").as_deref()==Ok("1"))
        .then(||std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{tag}-gains.jsonl"))).unwrap()));
    e.set_program_codec(codec.clone());
    e.set_direct_objects(true).unwrap();
    e.set_directional_hrtf(true);
    e.direct_mix = 1.;
    e.output_active = true;
    e.paused = false;
    let fifo = stereo_fifo::StereoFifo::new(4096);
    let telemetry = RuntimeTelemetry::default();
    let mut input = std::io::BufReader::new(std::fs::File::open(dir.join("sources.pcm")).unwrap());
    let mut output =
        std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{tag}.f32"))).unwrap());
    let mut targets = std::collections::BTreeMap::<u32, NativeObjectEvent>::new();
    let mut count = 0;
    let mut guard = 1.0f32;
    let mut labels = None;
    for line in
        std::io::BufReader::new(std::fs::File::open(dir.join("frames.jsonl")).unwrap()).lines()
    {
        let row: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let start = row["start"].as_u64().unwrap();
        if start >= seconds * 48000 {
            break;
        }
        assert_eq!(start, e.sample_pos);
        let n = row["samples"].as_u64().unwrap() as usize;
        if let Some(before) = &labels {
            assert_eq!(before, &row["labels"]);
        } else {
            labels = Some(row["labels"].clone());
        }
        let mut entries = Vec::new();
        for (ch, label) in row["labels"].as_array().unwrap().iter().enumerate() {
            let label = label.as_str().unwrap();
            let object = row["objectChannels"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["channel"].as_u64() == Some(ch as u64))
                .map(|o| o["id"].as_u64().unwrap() as u32)
                .or_else(|| label.strip_prefix("Obj_").and_then(|v| v.parse().ok()));
            let id = object.map_or_else(|| format!("bed:{ch}"), |id| format!("obj:{id}"));
            if count == 0 {
                let mut source = Source {
                    kind: if object.is_some() {
                        SourceKind::Object
                    } else {
                        SourceKind::Bed
                    },
                    bed_label: object.is_none().then(|| label.to_owned()),
                    object_id: object,
                    gain: 1.,
                    target_gain: 1.,
                    availability: 1.,
                    availability_target: 1.,
                    ..Default::default()
                };
                if object.is_none() {
                    Engine::set_source_route(&mut source, bed_route(label, &e.vbap), 0);
                }
                e.sources.insert(id.clone(), source);
                e.route_source_now(&id, 0).unwrap();
            }
            let mut bytes = vec![0; n * 4];
            input.read_exact(&mut bytes).unwrap();
            let pcm = bytes
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            entries.push((id, pcm));
        }
        let raw: Vec<NativeObjectEvent> = serde_json::from_value(row["events"].clone()).unwrap();
        let mut events = Vec::new();
        for event in raw {
            let redundant = targets.get(&event.id).is_some_and(|previous| {
                let ramp = if previous.ramp_duration == 0 {
                    128
                } else {
                    previous.ramp_duration
                };
                let mut a = previous.clone();
                let mut b = event.clone();
                a.sample_pos = 0;
                b.sample_pos = 0;
                a.ramp_duration = 0;
                b.ramp_duration = 0;
                a == b && previous.sample_pos + u64::from(ramp) <= event.sample_pos
            });
            if !redundant {
                targets.insert(event.id, event.clone());
                events.push(event);
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        assert!(protocol::apply_render_command(
            &mut e,
            render_command::RenderCommand::PcmFrameWithAck {
                start,
                entries,
                events,
                reply: tx
            },
            &fifo,
            &telemetry
        ));
        assert!(rx.recv().unwrap());
        let mut pcm = vec![0.; n * 2];
        e.render_into(&mut pcm, 2);
        assert!(pcm.iter().all(|v| v.is_finite()));
        for value in pcm {
            output.write_all(&value.to_le_bytes()).unwrap();
        }
        if let Some(trace)=&mut gain_trace {
            writeln!(trace,"{}",serde_json::json!({"sample":e.sample_pos,
                "programmeGainDb":20.*f64::from(e.program_gain).max(1e-12).log10(),
                "auxiliaryEnabled":e.auxiliary_balance.enabled(),"groupLevels":e.auxiliary_balance.levels()})).unwrap();
        }
        guard = guard.min(e.peak_guard.diagnostic_gain());
        count += 1;
    }
    output.flush().unwrap();
    if !unity { assert_eq!(guard, 1., "diagnostic headroom must keep peak protection inactive"); }
    std::fs::write(dir.join(format!("{tag}.json")),serde_json::to_vec_pretty(&serde_json::json!({"codec":codec,"layout":e.layout.as_str(),"frames":count,"samples":e.sample_pos,"sources":e.sources.len(),"outputDynamicsEnabled":false,"auxiliaryBalanceMode":"foreground-priority-v1","programmeGainDb":programme_db,"maximumAuxiliaryAttenuationDb":e.auxiliary_balance.maximum_attenuation_db,"unityOutput":unity,"minimumGuard":guard,"sharedClarity":e.active_hrtf_set.as_ref().unwrap().shared_clarity_active(),"rearDb":e.spatial_layer.rear_db,"frontCommonEnabled":e.front_common.enabled,"frontCommonGrouped":e.front_common.grouped,"frontCommonGain":e.front_common.gain,"frontCommonPresenceDb":e.front_common.presence_db(),"frontCommonActive":e.front_common.center.is_some(),"auditionConfig":audition})).unwrap()).unwrap();
}
