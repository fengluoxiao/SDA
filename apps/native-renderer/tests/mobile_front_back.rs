//! Front/back regression for the actual mobile KU100 asset, not the legacy dense set.
//! Filter separation is necessary, but does not prove perceptual externalization.
use sda_native_renderer::{directional::Direction, hrtf::NativeHrtfSet, vbap::{LayoutId, MAX_BUS_COUNT}};

fn pair(set: &NativeHrtfSet, position: [f32; 3]) -> (Vec<f32>, Vec<f32>) {
    set.directional_dry_compact(Direction {
        position, head: None, diffuse: 0.0, horizontal_only: false,
        width: 0.0, height: 0.0, depth: 0.0,
    }, LayoutId::Dolby7_1_4, [1.0; MAX_BUS_COUNT], [0.0; MAX_BUS_COUNT]).unwrap()
}
fn set() -> NativeHrtfSet {
    NativeHrtfSet::load_calibrated(&std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mobile/assets/hrtf-mobile-direct/hrtf-set.json")).unwrap()
}
#[test]
fn mobile_front_and_rear_do_not_collapse_to_stereo_pan() {
    let set = set();
    for x in [-0.5, 0.0, 0.5] {
        let front = pair(&set, [x, 1.0, 0.0]);
        let rear = pair(&set, [x, -1.0, 0.0]);
        let a: Vec<_> = front.0.iter().chain(&front.1).map(|v| f64::from(*v)).collect();
        let b: Vec<_> = rear.0.iter().chain(&rear.1).map(|v| f64::from(*v)).collect();
        assert_eq!(a.len(), b.len());
        assert!(a.iter().chain(&b).all(|v| v.is_finite()));
        let energy = |v: &[f64]| v.iter().map(|x| x*x).sum::<f64>();
        assert!(energy(&a) > 1e-12 && energy(&b) > 1e-12);
        let correlation = a.iter().zip(&b).map(|(a,b)| a*b).sum::<f64>() / (energy(&a)*energy(&b)).sqrt();
        assert!(correlation.abs() < 0.95, "x={x}: collapsed front/rear filters {correlation}");
    }
}
#[test]
fn normalized_position_radius_is_not_authored_distance() {
    let set = set();
    // Do not invent metres from normalized room coordinates. Explicit distance
    // processing lives outside this direction-only HRIR lookup.
    for p in [[0.0,1.0,0.0], [0.0,-1.0,0.0], [-1.0,1.0,1.0]] {
        assert_eq!(pair(&set,p), pair(&set,p.map(|v| v*0.25)));
    }
}

#[test]
fn mobile_upper_and_lower_directions_are_distinct() {
    let set = set();
    let upper = pair(&set, [-0.5, 0.5, 0.7]);
    let lower = pair(&set, [-0.5, 0.5, -0.7]);
    let a: Vec<_> = upper.0.iter().chain(&upper.1).map(|v| f64::from(*v)).collect();
    let b: Vec<_> = lower.0.iter().chain(&lower.1).map(|v| f64::from(*v)).collect();
    let energy = |v: &[f64]| v.iter().map(|x| x*x).sum::<f64>();
    assert!(energy(&a) > 1e-12 && energy(&b) > 1e-12);
    let correlation = a.iter().zip(&b).map(|(a,b)| a*b).sum::<f64>() / (energy(&a)*energy(&b)).sqrt();
    assert!(correlation.abs() < 0.95, "collapsed upper/lower filters {correlation}");
}
