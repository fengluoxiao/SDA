//! Offline KU100 interpolation audit; no device output or playback changes.
use sda_native_renderer::{directional::Grid, hrtf::NativeHrtfSet};
use std::{fs, path::Path};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let manifest = Path::new(&args[1]);
    let out = Path::new(&args[2]); fs::create_dir_all(out).unwrap();
    let data: serde_json::Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    let set = NativeHrtfSet::load_calibrated(manifest).unwrap();
    let irs: Vec<_> = data["positions"].as_array().unwrap().iter().map(|p|
        set.nearest(p["azimuth"].as_f64().unwrap(), p["elevation"].as_f64().unwrap()).unwrap()).collect();
    let guarded = Grid::new_with_notch_guard(&irs, true);
    let ordinary = Grid::new_with_notch_guard(&irs, false);
    let mut rows = Vec::new();
    for (i, (az, el)) in [(30.0,0.0),(-30.0,0.0),(109.5,0.0),(-109.5,0.0),
        (30.0,30.0),(-30.0,30.0),(109.5,30.0),(-109.5,30.0),(30.0,-30.0),(-30.0,-30.0)]
        .into_iter().enumerate() {
        for (name, grid) in [("guarded", &guarded), ("ordinary", &ordinary)] {
            let (l,r)=grid.interpolate(&irs,az,el);
            let samples: Vec<u8>=l.iter().chain(&r).flat_map(|s|s.to_le_bytes()).collect();
            let file=format!("obj{i}-{name}.f32");fs::write(out.join(&file),samples).unwrap();
            rows.push(serde_json::json!({"object":i,"azimuth":az,"elevation":el,"variant":name,"file":file,"samplesPerEar":l.len()}));
        }
    }
    fs::write(out.join("filters.json"),serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
}
