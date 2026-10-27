use super::*;

// Exercise the ordinary HRTF -> stereo FIFO used when system spatial is off.
fn render(enabled: bool, toggle: bool) -> (Vec<f32>, f64) {
    render_kind(enabled, toggle, false)
}
fn render_kind(enabled: bool, toggle: bool, objects: bool) -> (Vec<f32>, f64) {
    render_scene_codec(enabled,toggle,objects,"av3a",true)
}
fn render_scene_codec(enabled: bool, toggle: bool, objects: bool, codec: &'static str, spatial:bool) -> (Vec<f32>,f64) {
    render_source_scene(enabled,toggle,objects,codec,spatial,None)
}
fn render_source_scene(enabled: bool, toggle: bool, objects: bool, codec: &'static str, spatial:bool, lanes:Option<(usize,usize)>) -> (Vec<f32>,f64) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let active: Value = serde_json::from_slice(
        &std::fs::read(root.join("apps/mobile/ios-experiments/active.json")).unwrap(),
    )
    .unwrap();
    let asset = root
        .join(active["assetFamily"].as_str().unwrap())
        .join("hrtf-dense/hrtf-set.json");
    let path = CString::new(asset.to_str().unwrap()).unwrap();
    let config = CString::new(r#"{"sampleRate":48000,"outputChannels":2,"layout":"7.1.4","directObjectHrtf":true,"directionalHrtf":true,"hrtfWetWeight":0.0}"#).unwrap();
    let mut error = std::ptr::null_mut();
    let h = unsafe { sda_ios_create(config.as_ptr(), path.as_ptr(), &mut error) };
    assert!(!h.is_null(), "native create failed");
    struct Close(*mut std::ffi::c_void);
    impl Drop for Close {
        fn drop(&mut self) {
            unsafe {
                sda_ios_close(self.0);
            }
        }
    }
    let _close = Close(h);
    let host = unsafe { &*h.cast::<Host>() };
    // Synthetic frame injection bypasses Engine::feed; announce the same codec
    // that the real feeder declares before delivering its decoded frames.
    {
        let engine = host.engine.lock().unwrap();
        assert!(engine.pipeline.as_ref().unwrap().commands.push(
            render_command::RenderCommand::Command(Command::SetProgramCodec { codec: codec.into() })
        ).is_ok());
    }
    host.engine.lock().unwrap().set_spatial_enhancement(spatial).unwrap();
    // Use the same explicit preamp in both linear-ratio probes. Unlike the
    // listener slider, it is included in the shared final-output loudness meter.
    host.engine.lock().unwrap().set_master_preamp_db(-12.).unwrap();
    host.engine.lock().unwrap().set_volume(0.5).unwrap();
    host.engine
        .lock()
        .unwrap()
        .set_volume_balance(enabled)
        .unwrap();
    let private = (objects && lanes.is_none())
        .then(|| std::env::var("SDA_OBJECT_BALANCE_INPUT").ok())
        .flatten();
    let frames = if private.is_some() {
        std::env::var("SDA_OBJECT_BALANCE_SAMPLES")
            .unwrap()
            .parse::<usize>()
            .unwrap()
    } else if lanes.is_some() { 48000 } else {
        48000 * if toggle { 20 } else { 15 }
    };
    // A real decoded excerpt can be supplied for diagnostic regression.
    let pcm = (!toggle)
        .then(|| std::env::var("SDA_AV3A_BALANCE_PCM").ok())
        .flatten()
        .map(|p| {
            std::fs::read(p)
                .unwrap()
                .chunks_exact(4)
                .map(|s| f32::from_le_bytes(s.try_into().unwrap()))
                .collect::<Vec<_>>()
        });
    std::thread::scope(|scope| {
        let sink = host.output.clone();
        let (requests, receiver) = std::sync::mpsc::channel::<bool>();
        let (ack, acknowledgements) = std::sync::mpsc::channel();
        let consumer = scope.spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(120);
            let mut output = Vec::new();
            let mut toggles = 0;
            while output.len() < frames * 2 {
                let mut l = [0.0; 512];
                let mut r = [0.0; 512];
                let n = pull(&sink, &mut l, &mut r);
                for i in 0..n {
                    output.extend_from_slice(&[l[i], r[i]]);
                }
                if toggle && toggles < 2 && output.len() >= (12 + toggles * 4) * 48000 * 2 {
                    requests.send(toggles == 1).unwrap();
                    acknowledgements
                        .recv_timeout(Duration::from_secs(30))
                        .unwrap();
                    toggles += 1;
                }
                assert!(
                    Instant::now() < deadline,
                    "AV3A native output stalled at {}",
                    output.len() / 2
                );
                if n == 0 {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            output.truncate(frames * 2);
            output
        });
        let service = || {
            while let Ok(enabled) = receiver.try_recv() {
                host.engine
                    .lock()
                    .unwrap()
                    .set_volume_balance(enabled)
                    .unwrap();
                ack.send(()).unwrap();
            }
        };
        // Synthetic injection bypasses the app feeder. Respect the same bounded
        // decode-ahead horizon, including while a live toggle waits for service.
        // FIFO-only throttling misses work queued before the renderer catches up.
        let feed_deadline=Instant::now()+Duration::from_secs(120);
        let pace=|next_sample:u64| {
            loop {
                service();
                let engine=host.engine.lock().unwrap();
                let started=engine.startup.lock().unwrap().started;
                let consumed=engine.playback_status().consumed_sample_pos;
                drop(engine);
                if !started || next_sample<=consumed+2*48000 {break;}
                assert!(Instant::now()<feed_deadline,"bounded AV3A feeder stalled");
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        let gain = {
            let mut reference_meter = balance::LoudnessMeter::speakers_7_1_4();
            let mut scene_meter=sda_core::scene_reference::SceneMeter::new();
            if let Some(path) = private.clone().or_else(|| {
                (!toggle)
                    .then(|| std::env::var("SDA_AV3A_BALANCE_BITSTREAM").ok())
                    .flatten()
            }) {
                let bytes = std::fs::read(path).unwrap();
                for chunk in bytes.chunks(2415 * 8) {
                    let next_sample=host.engine.lock().unwrap().decoded_sample_pos();
                    pace(next_sample);
                    service();
                    let status = host.engine.lock().unwrap().feed(chunk).unwrap();
                    assert!(status.errors.is_empty(), "{:?}", status.errors);
                }
                host.engine.lock().unwrap().finish().unwrap();
            } else {
                for start in (0..frames).step_by(4096) {
                    pace(start as u64);
                    service();
                    let count = 4096.min(frames - start);
                    let channels = (0..12)
                        .map(|ch| {
                            (start..start + count)
                                .map(|i| {
                                    pcm.as_ref().map_or_else(
                                        || {
                                            if ch == 3 {
                                                if codec=="ac4" {0.08*(i as f32*std::f32::consts::TAU*80./48000.).sin()} else {0.0}
                                            } else {
                                                0.12 * (i as f32 * (0.04 + ch as f32 * 0.007)).sin()
                                            }
                                        },
                                        |p| p[(i % (p.len() / 12)) * 12 + ch],
                                    )
                                })
                                .collect()
                        })
                        .collect();
                    let mut frame = sda_core::FrameData {
                        reference_loudness: None,
                        compatibility_bed: None,
                        codec,
                        sample_rate: 48000,
                        sample_pos: start as u64,
                        channels,
                        labels: [
                            "L", "R", "C", "LFE", "Lb", "Rb", "Ls", "Rs", "Tfl", "Tfr", "Tbl",
                            "Tbr",
                        ]
                        .map(String::from)
                        .into(),
                        raw_bed_labels: vec![],
                        events: vec![],
                        object_channels: vec![],
                        program_loudness: None,
                        ramp_duration: 0,
                    };
                    if objects {
                        reference_meter.push(&frame.channels);
                        frame.reference_loudness = Some(reference_meter.integrated());
                        let dirs: [(f64, f64); 12] = [
                            (30., 0.),
                            (-30., 0.),
                            (0., 0.),
                            (0., 0.),
                            (135., 0.),
                            (-135., 0.),
                            (90., 0.),
                            (-90., 0.),
                            (45., 45.),
                            (-45., 45.),
                            (135., 45.),
                            (-135., 45.),
                        ];
                        for ch in (0..12).filter(|ch| *ch != 3) {
                            let id = 100 + ch as u32;
                            frame.labels[ch] = format!("Obj_{id}");
                            frame.object_channels.push(sda_core::ObjectChannelDecl {
                                id,
                                channel: ch as u32,
                            });
                            if start == 0 {
                                let (az, el) = (dirs[ch].0.to_radians(), dirs[ch].1.to_radians());
                                frame.events.push(sda_core::ObjectEvent { zone_exclusion: vec![], horizontal_only: false,
                                    id,
                                    sample_pos: 0,
                                    has_pos: true,
                                    pos: [-az.sin() * el.cos(), az.cos() * el.cos(), el.sin()],
                                    gain_db: 0.,
                                    size: [0.; 3],
                                    diffuse: 0.,
                                    anchor: "room".into(),
                                    distance_m: None,
                                    distance_infinite: false,
                                    screen_factor: None,
                                    depth_factor: None,
                                    ramp_duration: 1,
                                });
                            }
                        }
                        frame.raw_bed_labels = vec!["LFE".into()];
                    }
                    if let Some((total,hot))=lanes {
                        frame.channels=(0..total).map(|lane|(start..start+count).map(|i|if lane==hot {0.05*(i as f32*std::f32::consts::TAU*997.0/48000.0).sin()}else{0.0}).collect()).collect();
                        frame.labels=(0..total).map(|i|format!("Obj_{}",1000+i)).collect();
                        frame.object_channels=(0..total).map(|i|sda_core::ObjectChannelDecl{id:1000+i as u32,channel:i as u32}).collect();
                        frame.raw_bed_labels.clear();frame.events.clear();
                        if start==0 { for lane in 0..total {frame.events.push(sda_core::ObjectEvent { zone_exclusion: vec![], horizontal_only: false,
                            id:1000+lane as u32,sample_pos:0,has_pos:true,pos:[0.0,1.0,0.0],gain_db:0.0,size:[0.0;3],diffuse:0.0,
                            anchor:"room".into(),distance_m:None,distance_infinite:false,screen_factor:None,depth_factor:None,ramp_duration:1,
                        });} }
                    }
                    if objects || codec!="av3a" {
                        frame.reference_loudness=Some(scene_meter.measure(&frame));
                    }
                    let mut engine = host.engine.lock().unwrap();
                    let pending = engine.balance.measure(frame, None);
                    engine.pending.lock().unwrap().push_back(pending);
                    engine.drain_pending_into_pipeline().unwrap();
                }
            }
            let mut engine = host.engine.lock().unwrap();
            engine.startup.lock().unwrap().ended = true;
            engine.start_playback_if_ready().unwrap();
            engine.balance.gain_db
        };
        while let Ok(enabled) = receiver.recv() {
            host.engine
                .lock()
                .unwrap()
                .set_volume_balance(enabled)
                .unwrap();
            ack.send(()).unwrap();
        }
        let output = consumer.join().unwrap();
        let status = host.engine.lock().unwrap().playback_status();
        assert_eq!(
            status.hrtf_directions as u64,
            active["directionCount"].as_u64().unwrap()
        );
        assert!(status.volume_balance_eligible);
        assert_eq!(status.volume_balance_enabled, enabled);
        assert_eq!(status.renderer_program_enabled, enabled);
        if enabled {
            assert!(if private.is_some() {
                status.rendered_program_gain_db <= 0.001
            } else {
                status.rendered_program_gain_db < -3.0
            });
        } else {
            assert_eq!(status.rendered_program_gain_db, 0.0);
        }
        (output, gain)
    })
}
#[test]
fn av3a_native_balance_reaches_hrtf_output_without_system_spatial() {
    let (off, off_gain) = render(false, false);
    let (on, on_gain) = render(true, false);
    assert!(
        (off_gain - on_gain).abs() < 0.1,
        "streaming targets diverged: {off_gain} / {on_gain}"
    );
    let energy = |p: &[f32]| p.iter().map(|s| (*s as f64).powi(2)).sum::<f64>();
    let first = 48000 * 2..48000 * 3 * 2;
    let startup_db = 10.0 * (energy(&on[first.clone()]) / energy(&off[first])).log10();
    eprintln!("AV3A first audible segment: {startup_db:.3} dB");
    assert!(startup_db < -3.0, "startup escaped balance: {startup_db}");
    let range = 48000 * 12 * 2..48000 * 14 * 2;
    let ratio = energy(&on[range.clone()]) / energy(&off[range]);
    let actual_db = 10.0 * ratio.log10();
    eprintln!("AV3A native HRTF balance: target={on_gain:.3} dB, output={actual_db:.3} dB");
    assert!(on_gain < -3.0);
    if std::env::var_os("SDA_AV3A_BALANCE_PCM").is_some()
        || std::env::var_os("SDA_AV3A_BALANCE_BITSTREAM").is_some()
    {
        // On a hot real mix the bypass can engage the peak guard. It cannot be
        // used as a linear unity reference; the output must still attenuate.
        assert!(
            actual_db < -3.0,
            "real AV3A PCM was not attenuated: {actual_db}"
        );
    } else {
        assert!(
            (actual_db - (on_gain + 1.5)).abs() < 0.05,
            "ordinary HRTF gain missing: {actual_db} versus {on_gain}"
        );
    }
}

#[test]
fn av3a_native_live_balance_switch_changes_the_same_playback_output() {
    let (pcm, gain) = render(true, true);
    let energy = |start: usize, end: usize| {
        let p = &pcm[start * 48000 * 2..end * 48000 * 2];
        p.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / p.len() as f64
    };
    let before = energy(10, 11);
    let disabled = energy(14, 15);
    let restored = energy(18, 19);
    let off_delta = 10.0 * (disabled / before).log10();
    let on_delta = 10.0 * (restored / disabled).log10();
    eprintln!("AV3A live switch: target={gain:.3} dB, off delta={off_delta:.3} dB, on delta={on_delta:.3} dB");
    assert!(
        (off_delta + gain + 1.5).abs() < 0.2,
        "live switch off ineffective: {off_delta}"
    );
    assert!(
        (on_delta - gain - 1.5).abs() < 0.2,
        "live switch on ineffective: {on_delta}"
    );
}

#[test]
fn av3a_object_reference_balance_reaches_hrtf_and_live_switch() {
    let (off, _) = render_kind(false, false, true);
    let (on, gain) = render_kind(true, false, true);
    let energy = |p: &[f32]| p.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>();
    let range = 12 * 48000 * 2..14 * 48000 * 2;
    let db = 10. * (energy(&on[range.clone()]) / energy(&off[range])).log10();
    assert!(gain < -3.);
    assert!((db - gain - 1.5).abs() < 0.1, "object target {gain}, output {db}");
    let (live, gain) = render_kind(true, true, true);
    let energy_at = |a: usize, b: usize| {
        energy(&live[a * 48000 * 2..b * 48000 * 2]) / ((b - a) * 48000 * 2) as f64
    };
    let delta_off = 10. * (energy_at(14, 15) / energy_at(10, 11)).log10();
    let delta_on = 10. * (energy_at(18, 19) / energy_at(14, 15)).log10();
    assert!(
        (delta_off + gain + 1.5).abs() < 0.2,
        "object live OFF {delta_off}, target {gain}"
    );
    assert!(
        (delta_on - gain - 1.5).abs() < 0.2,
        "object live ON {delta_on}, target {gain}"
    );
}
#[test]
#[ignore = "requires SDA_OBJECT_BALANCE_INPUT and SDA_OBJECT_BALANCE_SAMPLES; private audio is not committed"]
fn av3a_private_object_programme_balance_changes_real_stereo_output() {
    let (off, off_gain) = render_kind(false, false, true);
    let (on, on_gain) = render_kind(true, false, true);
    assert!(
        (off_gain - on_gain).abs() < 0.02,
        "switch changed the reference measurement"
    );
    assert!(on_gain <= 0., "programme balance must not boost: {on_gain}");
    let expect_cut = std::env::var("SDA_OBJECT_EXPECT_ATTENUATION").is_ok_and(|s| s == "1");
    if expect_cut {
        assert!(
            on_gain < -0.5,
            "expected real-file attenuation, got {on_gain}"
        );
    }
    let energy = |p: &[f32]| p.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>();
    let mut windows = vec![];
    for (start, end) in [(12usize, 14usize), (76, 90), (150, 180), (180, 200)] {
        if end * 48000 * 2 > off.len() {
            continue;
        }
        let range = start * 48000 * 2..end * 48000 * 2;
        let delta = 10. * (energy(&on[range.clone()]) / energy(&off[range])).log10();
        assert!(
            delta <= 0.02,
            "balance boosted real output at {start}: {delta}"
        );
        windows.push(serde_json::json!({"start":start,"end":end,"onMinusOffDb":delta}));
    }
    println!("real stereo windows: {windows:?}, final target {on_gain}");
    if expect_cut {
        assert!(
            windows
                .iter()
                .any(|v| v["onMinusOffDb"].as_f64().unwrap() < -0.05),
            "gain status alone does not prove audible output attenuation"
        );
    }
    let input = std::env::var("SDA_OBJECT_BALANCE_INPUT").unwrap();
    let report = serde_json::json!({"input":input,"samples":off.len()/2,"targetDb":on_gain,"windows":windows,"offGainDb":off_gain});
    println!("{report}");
    if let Ok(folder) = std::env::var("SDA_OBJECT_BALANCE_REPORT_DIR") {
        let root = std::path::Path::new(&folder);
        std::fs::create_dir_all(root).unwrap();
        let tag = std::path::Path::new(&input)
            .file_stem()
            .unwrap()
            .to_string_lossy();
        std::fs::write(
            root.join(format!("{tag}-balance.json")),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn shared_object_codecs_four_state_matrix() {
    let energy=|pcm:&[f32]| pcm[12*48000*2..14*48000*2].iter()
        .map(|v|f64::from(*v).powi(2)).sum::<f64>();
    for (codec,objects) in [("av3a",false),("av3a",true),("ac4",true)] {
        for spatial in [false,true] {
            let (off,off_gain)=render_scene_codec(false,false,objects,codec,spatial);
            let (on,gain)=render_scene_codec(true,false,objects,codec,spatial);
            assert!(gain < -3.,"hot scene must actually request programme attenuation");
            assert!((off_gain-gain).abs()<0.1);
            let actual=10.*(energy(&on)/energy(&off)).log10();
            eprintln!("four-state matrix codec={codec} objects={objects} spatial={spatial} target={gain:.3} output={actual:.3}");
            assert!((actual-(gain+1.5)).abs()<0.15,"{codec}: programme attenuation/calibration mismatch: {actual} vs {}",gain+1.5);
        }
    }
}

#[test]
fn ac4_decoder_volume_spatial_four_state_matrix() {
 let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
 let active:Value=serde_json::from_slice(&std::fs::read(root.join("apps/mobile/ios-experiments/active.json")).unwrap()).unwrap();
 let asset=root.join(active["assetFamily"].as_str().unwrap()).join("hrtf-dense/hrtf-set.json");
 let path=CString::new(asset.to_str().unwrap()).unwrap();
 let config=CString::new(r#"{"sampleRate":48000,"outputChannels":2,"layout":"7.1.4","directObjectHrtf":true,"directionalHrtf":true,"hrtfWetWeight":0.0}"#).unwrap();
 let bytes=include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../../packages/core/tests/fixtures/ac4-minimal-full.ac4"));
 for volume in [false,true] {for spatial in [false,true] {
  let mut error=std::ptr::null_mut();let h=unsafe{sda_ios_create(config.as_ptr(),path.as_ptr(),&mut error)};assert!(!h.is_null());
  struct Close(*mut std::ffi::c_void);impl Drop for Close {fn drop(&mut self){unsafe{sda_ios_close(self.0);}}}
  let _close=Close(h);let host=unsafe{&*h.cast::<Host>()};
  let expected={let mut e=host.engine.lock().unwrap();e.set_volume_balance(volume).unwrap();e.set_spatial_enhancement(spatial).unwrap();
   for chunk in bytes.chunks(17) {let result=e.feed(chunk).unwrap();assert!(result.errors.is_empty(),"{:?}",result.errors);}
   e.finish().unwrap();e.decoded_sample_pos()};
  assert!(expected>0);let mut count=0;let deadline=Instant::now()+Duration::from_secs(20);
  while count<expected {
   let (mut l,mut r)=([0.;512],[0.;512]);let n=pull(&host.output,&mut l,&mut r);
   assert!(l.iter().chain(&r).all(|v|v.is_finite()));count+=n as u64;
   assert!(Instant::now()<deadline,"AC4 decoder matrix stalled");if n==0 {std::thread::sleep(Duration::from_millis(1));}
  }
  let status=host.engine.lock().unwrap().playback_status();
  assert_eq!(status.volume_balance_enabled,volume);assert!(status.volume_balance_eligible);
  assert_eq!(status.renderer_program_enabled,volume);assert_eq!(status.spatial_enhancement_enabled,spatial);
  eprintln!("AC4 actual decoder matrix volume={volume} spatial={spatial} frames={count}");
 }}
}

#[test]
fn all_object_codecs_128th_lane_reaches_actual_stereo_fifo() {
    for codec in ["ac4","eac3","mpegh","av3a"] {
        for spatial in [false,true] {
            let (first,_)=render_source_scene(false,false,true,codec,spatial,Some((128,0)));
            let (last,_)=render_source_scene(false,false,true,codec,spatial,Some((128,127)));
            assert_eq!(first.len(),48000*2);assert_eq!(last.len(),first.len());
            assert!(last.iter().all(|v|v.is_finite()));
            let energy=|pcm:&[f32]|pcm.iter().map(|v|f64::from(*v).powi(2)).sum::<f64>();
            let baseline=energy(&first);let terminal=energy(&last);
            assert!(baseline>0.01&&terminal>0.01,"{codec}: terminal source was silent");
            let delta=10.0*(terminal/baseline).log10();
            assert!(delta.abs()<0.01,"{codec}: source 127 was changed/lost: {delta} dB");
        }
    }
}
