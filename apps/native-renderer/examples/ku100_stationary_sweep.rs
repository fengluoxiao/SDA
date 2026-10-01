//! Diagnostic-only stationary-direction sweep. Filters are exported before the master limiter.
use sda_native_renderer::{directional::{Direction, Grid}, hrtf::NativeHrtfSet, vbap::{self, LayoutId, VbapSolver}};
use std::{fs, io::Write, path::Path};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let manifest = Path::new(&args[1]);
    let out = Path::new(&args[2]);
    fs::create_dir_all(out).unwrap();
    let data: serde_json::Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    let mut set = NativeHrtfSet::load_calibrated(manifest).unwrap();
    let irs: Vec<_> = data["positions"].as_array().unwrap().iter().map(|p|
        set.nearest(p["azimuth"].as_f64().unwrap(), p["elevation"].as_f64().unwrap()).unwrap()).collect();
    let guarded = Grid::new_with_notch_guard(&irs, true);
    let ordinary = Grid::new_with_notch_guard(&irs, false);
    let mut rows = Vec::new();
    let mut file = std::io::BufWriter::new(fs::File::create(out.join("filters.f32")).unwrap());
    let mut offset = 0usize;
    for layout in [LayoutId::Sony360Ra13, LayoutId::Dolby7_1_4] {
        let solver = VbapSolver::with_layout(layout);
        let speakers: Vec<_> = vbap::speakers(layout).iter().map(|s|
            set.mixed_speaker(s.name, layout.as_str(), s.azimuth as f64, s.elevation as f64, 0.0).unwrap()).collect();
        for el in [-30.0_f64, 0.0, 30.0, 45.0] {
            for az in (-180..180).step_by(2).map(|a| a as f64).chain([109.5, -109.5, 30.0, -30.0]) {
                let a=az.to_radians();let e=el.to_radians();
                let position=[(-e.cos()*a.sin()) as f32, (e.cos()*a.cos()) as f32, e.sin() as f32];
                let gains=solver.pan(position,0.0);
                let direction=Direction {position,head:None,diffuse:0.0,horizontal_only:false,width:0.0,height:0.0,depth:0.0};
                let mut mixed=(vec![0.0;set.speaker_filter_len()],vec![0.0;set.speaker_filter_len()]);
                for (bus,(l,r)) in speakers.iter().enumerate() {
                    for (output,input) in [(&mut mixed.0,l),(&mut mixed.1,r)] {
                        for (o,v) in output.iter_mut().zip(input) { *o+=gains[bus]*v; }
                    }
                }
                for (mode,(l,r)) in [
                    ("guarded",guarded.interpolate(&irs,az,el)),
                    ("ordinary",ordinary.interpolate(&irs,az,el)),
                    ("continuous",set.directional_dry_compact(direction,layout,gains,[0.0;vbap::MAX_BUS_COUNT]).unwrap()),
                    ("speakers",mixed),
                ] {
                    assert_eq!(l.len(),r.len());
                    for v in l.iter().chain(&r) {file.write_all(&v.to_le_bytes()).unwrap();}
                    rows.push(serde_json::json!({"layout":layout.as_str(),"az":az,"el":el,"mode":mode,"offset":offset,"length":l.len()}));
                    offset+=l.len()*2;
                }
            }
        }
    }
    file.flush().unwrap();
    fs::write(out.join("filters.json"),serde_json::to_vec(&rows).unwrap()).unwrap();
}
