//! Portable, silent DSP workload probes. This is not a device/decoder emulator.
use super::*;
use serde_json::{Value, json};
use std::time::Instant;

pub fn calibration() -> Value {
    // Versioned independent kernel: renderer optimizations must not change the scale.
    let mut trials = Vec::new();
    for _ in 0..5 {
        let mut memory = vec![1u64; 32768];
        let mut x = 17u64;
        let at = Instant::now();
        for i in 0..2_000_000usize {
            let j = (x as usize) & 32767;
            x = x.wrapping_mul(6364136223846793005).wrapping_add(memory[j]);
            memory[j] = x.rotate_left((i & 63) as u32);
            std::hint::black_box(x);
        }
        std::hint::black_box(memory);
        trials.push(at.elapsed().as_secs_f64() * 1000.0);
    }
    trials.sort_by(f64::total_cmp);
    json!({"kernel":"sda-integer-memory-v1","medianMs":trials[2],"minMs":trials[0],"maxMs":trials[4],"platform":std::env::consts::OS,"arch":std::env::consts::ARCH,"logicalCpus":std::thread::available_parallelism().map(|x|x.get()).unwrap_or(1)})
}

pub fn describe(e: &Engine) -> Value {
    let shape = e.active_hrtf_set.as_ref().map(|s| s.simulation_shape());
    let sources:Vec<_>=e.sources.values().map(|s|json!({"kind":if s.kind==SourceKind::Object{"object"}else{"bed"},"bedLabel":s.bed_label,"position":s.position,"spread":s.spread,"extent":s.extent,"diffuse":s.diffuse,"muted":s.muted})).collect();
    json!({"schema":1,"time":performance::now_us()/1000,"sampleRate":48000,"layout":e.layout.as_str(),"paused":e.paused,"outputActive":e.output_active,"directObjects":e.direct_objects,"directionalHrtf":e.directional_hrtf,"hrtfShape":shape,"sources":sources,"routeUpdates":e.route_update_count,"roomEnabled":e.cinema.enabled,"hardwareEnabled":e.cinema.monitor.hardware.enabled,"nearField":{"enabled":e.near_field.enabled,"metresPerUnit":e.near_field.metres_per_unit},"sourceExtent":{"enabled":e.source_extent.enabled,"width":e.source_extent.width,"diffusion":e.source_extent.diffusion}})
}

fn simulate(v: &Value) -> Result<Value, String> {
    if v["schema"] != 1 || v["sampleRate"] != 48000 {
        return Err("unsupported workload schema/sample rate".into());
    }
    let sources = v["sources"].as_array().ok_or("sources required")?;
    if sources.len() > MAX_SOURCES {
        return Err("source count exceeds supported limit".into());
    }
    let shape: [usize; 3] =
        serde_json::from_value(v["hrtfShape"].clone()).map_err(|_| "HRTF shape unavailable")?;
    let mut e = Engine::new(48000, 2);
    e.active_hrtf_set = Some(hrtf::NativeHrtfSet::synthetic(
        shape[0], shape[1], shape[2],
    )?);
    e.set_layout(
        vbap::LayoutId::parse(v["layout"].as_str().unwrap_or("")).ok_or("unsupported layout")?,
    )?;
    e.near_field =
        serde_json::from_value(v["nearField"].clone()).map_err(|_| "invalid near field")?;
    e.source_extent =
        serde_json::from_value(v["sourceExtent"].clone()).map_err(|_| "invalid extent")?;
    if !e.near_field.valid() || !e.source_extent.valid() {
        return Err("invalid spatial settings".into());
    }
    e.directional_hrtf = v["directionalHrtf"]
        .as_bool()
        .ok_or("missing directional flag")?;
    // Room assets, hardware models and user coefficients are deliberately not impersonated.
    e.cinema.enabled = false;
    e.rebuild_bus_renderer()?;
    e.output_active = true;
    for (i, s) in sources.iter().enumerate() {
        let position: [f32; 3] =
            serde_json::from_value(s["position"].clone()).map_err(|_| "invalid position")?;
        if !position.iter().all(|x| x.is_finite() && x.abs() <= 10000.0) {
            return Err("invalid position".into());
        }
        let kind = match s["kind"].as_str() {
            Some("object") => SourceKind::Object,
            Some("bed") => SourceKind::Bed,
            _ => return Err("invalid source kind".into()),
        };
        let extent: [f32; 3] =
            serde_json::from_value(s["extent"].clone()).map_err(|_| "invalid source extent")?;
        let spread = s["spread"].as_f64().ok_or("invalid spread")? as f32;
        let diffuse = s["diffuse"].as_f64().ok_or("invalid diffuse")? as f32;
        if !extent
            .iter()
            .chain([&spread, &diffuse])
            .all(|x| x.is_finite() && *x >= 0.0 && *x <= 1.0)
        {
            return Err("unsupported spatial range".into());
        }
        let source = Source {
            kind,
            bed_label: s["bedLabel"].as_str().map(str::to_owned),
            position,
            extent,
            spread,
            diffuse,
            gain: 1.0,
            target_gain: 1.0,
            availability: 1.0,
            availability_target: 1.0,
            muted: s["muted"].as_bool().unwrap_or(false),
            ..Source::default()
        };
        let id = format!("probe:{i}");
        e.sources.insert(id.clone(), source);
        e.route_source_now(&id, 0)?;
    }
    e.set_direct_objects(v["directObjects"].as_bool().ok_or("missing direct flag")?)?;
    let n = convolution::DEFAULT_PARTITION;
    let signal: Vec<f32> = (0..n)
        .map(|i| ((i * 37 % 97) as f32 - 48.0) * 0.0001)
        .collect();
    let mut output = vec![0.0; n * 2];
    let mut times = Vec::new();
    for i in 0..128 {
        let pos = e.sample_pos;
        for s in e.sources.values_mut() {
            s.samples.write(pos, pos, &signal);
        }
        let at = Instant::now();
        e.render_into(&mut output, 2);
        let ms = at.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&output);
        if i >= 16 {
            times.push(ms);
        }
    }
    times.sort_by(f64::total_cmp);
    Ok(
        json!({"meanBlockMs":times.iter().sum::<f64>()/times.len() as f64,"p95BlockMs":times[times.len()*95/100],"maxBlockMs":times.last(),"blockFrames":n,"blockBudgetMs":n as f64/48.0,"sources":sources.len(),"scope":"steady-state synthetic DSP probe; independent snapshots, not continuous timeline replay","unsupported":["original bitstream decoder","exact room and user filter coefficients","motion between snapshots","device APIs and Bluetooth","GPU","network and queue scheduling"]}),
    )
}

pub fn entry() -> bool {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--sda-performance-calibrate") {
        println!("{}", calibration());
        return true;
    }
    if args.get(1).map(String::as_str) != Some("--sda-performance-simulate") {
        return false;
    }
    let result = (|| -> Result<Value, String> {
        let path = args.get(2).ok_or("workload file required")?;
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 2 * 1024 * 1024 {
            return Err("workload file too large".into());
        }
        let v: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        simulate(&v)
    })();
    match result {
        Ok(value) => println!("{value}"),
        Err(error) => {
            println!("{}", json!({"error":error}));
            std::process::exit(2)
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silent_probe_runs_real_engine_and_rejects_bad_schema() {
        let mut e = Engine::new(48000, 2);
        e.active_hrtf_set = Some(hrtf::NativeHrtfSet::synthetic(8, 128, 0).unwrap());
        e.sources.insert("obj:0".into(), Source::default());
        let v = describe(&e);
        let result = simulate(&v).unwrap();
        assert!(result["meanBlockMs"].as_f64().unwrap() > 0.0);
        assert!(simulate(&json!({"schema":99})).is_err());
    }
}
