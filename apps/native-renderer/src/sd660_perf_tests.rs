use crate::*;
#[test]
#[ignore = "same-input mobile-policy PCM/performance comparison"]
fn render_mobile_reference_scene() {
    use std::io::Write;
    let out=std::path::PathBuf::from(std::env::var("SDA_SD660_OUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let manifest=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
    for (codec,count,moving) in [("eac3",15,false),("eac3",15,true),("mpegh",15,true),("av3a",32,true)] {
        let mut e=Engine::new(48000,2);
        e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&manifest).unwrap(),0.).unwrap();
        if codec=="mpegh" {e.set_layout(vbap::LayoutId::Sony360Ra13).unwrap();}
        e.set_program_codec(codec.into());e.set_spatial_enhancement(true).unwrap();
        e.set_direct_objects(true).unwrap();e.set_directional_hrtf(true);
        e.program_enabled=true;e.set_program_target(1.,true);e.output_active=true;e.paused=false;e.direct_mix=1.;
        let ids:Vec<_>=(0..count).map(|i|format!("obj:{i}")).collect();
        for (i,id) in ids.iter().enumerate() {
            let angle=i as f32*std::f32::consts::TAU/count as f32;
            let s=Source {kind:SourceKind::Object,object_id:Some(i as u32),position:[angle.sin(),angle.cos(),(i%3)as f32*0.3],
                gain:1.,target_gain:1.,availability:1.,availability_target:1.,..Default::default()};
            e.sources.insert(id.clone(),s);e.route_source_now(id,0).unwrap();
        }
        let mut times=Vec::new();let mut pcm=Vec::new();let mut stage_total=[0u64;7];
        for block in 0..180 {
            let now=e.sample_pos;
            for (i,id) in ids.iter().enumerate() {
                let s=e.sources.get_mut(id).unwrap();
                let values:[f32;convolution::DEFAULT_PARTITION]=std::array::from_fn(|j| {
                    let at=now as usize+j;
                    let gate=if i%5==0 && (block/12)%3==1 {0.} else {1.};
                    (((at+i*97)as f32*(0.033+i as f32*0.002)).sin()*0.0007
                      +((at+i*11)as f32*0.31).sin()*0.00005)*gate
                });
                s.samples.write(now,now,&values);
                if moving {
                    let angle=i as f32*std::f32::consts::TAU/count as f32+block as f32*0.009;
                    s.position=[angle.sin(),angle.cos(),(i%3)as f32*0.3];
                    let buses=bus_renderer::route(&e.vbap,s.position,None,0.);
                    Engine::set_source_route(s,RouteGains{buses,lfe:0.},128);
                }
            }
            let mut output=[0.;convolution::DEFAULT_PARTITION*2];
            let begin=std::time::Instant::now();e.render_into(&mut output,2);
            let elapsed=begin.elapsed().as_micros()as u64;
            if block>=20 {times.push(elapsed);let t=e.last_render_stages;
                for (sum,value) in stage_total.iter_mut().zip([t.prepare,t.fast_mix,t.fast_reduce,t.sample,t.directional,t.direct,t.bus]){*sum+=value;}}
            pcm.extend_from_slice(&output);
        }
        times.sort_unstable();let name=format!("{codec}-{count}-{}",if moving{"moving"}else{"static"});
        let mut file=std::fs::File::create(out.join(format!("{name}.f32"))).unwrap();
        for x in pcm {file.write_all(&x.to_le_bytes()).unwrap();}
        let report=serde_json::json!({"codec":codec,"sources":count,"moving":moving,"meanMicros":times.iter().sum::<u64>()as f64/times.len()as f64,
            "p95Micros":times[times.len()*95/100],"maxMicros":times[times.len()-1],"audioBudgetMicros":1024.0/48000.0*1e6,"stagesMeanMicros":stage_total.map(|v|v as f64/times.len()as f64)});
        std::fs::write(out.join(format!("{name}.json")),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        eprintln!("SD660_POLICY {report}");
    }
}
