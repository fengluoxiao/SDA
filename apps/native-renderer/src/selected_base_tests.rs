//! Selected common base contracts: geometry, independent source gains and plain bypass.
use super::*;

#[test]
fn adaptive_base_has_no_static_positive_fallback_at_any_direction() {
    for rate in [44100,48000,96000] {
        let mut e=Engine::new(rate,2);
        e.set_spatial_enhancement(true).unwrap();
        assert_eq!(e.spatial_layer.geometry,spatial_layer::Geometry::Front45);
        assert_eq!(e.spatial_layer.rear_db,0.);
        for degrees in (-180..=180).step_by(5) {
            let a=(degrees as f64).to_radians();
            let position=[a.sin() as f32,a.cos() as f32,0.];
            let expected=1f32;
            let mut source=Source{kind:SourceKind::Object,position,gain:0.42,target_gain:0.42,..Default::default()};
            let mut previous=1.;
            for at in 0..rate/10 {
                let gain=e.spatial_layer.source_gain(&mut source,at as u64);
                assert!((gain-previous).abs()<=e.spatial_layer.slew+1e-6);
                previous=gain;
            }
            assert!((previous*e.spatial_preamp.gain()-expected).abs()<2e-6,"rate={rate}, azimuth={degrees}");
            assert_eq!(source.gain,0.42);assert_eq!(source.target_gain,0.42);
        }
    }
}

#[test]
fn optional_manual_layer_recomputes_weights_across_policy_and_layout_changes() {
    for layout in [vbap::LayoutId::Dolby7_1_4,vbap::LayoutId::Sony360Ra13] {
        let mut e=Engine::new(48000,2);e.set_layout(layout).unwrap();
        for label in ["FrontLeft","FrontRight","SurroundLeft","SurroundRight","LFE"] {
            e.sources.insert(label.into(),Source{kind:SourceKind::Bed,bed_label:Some(label.into()),..Default::default()});
        }
        e.set_spatial_enhancement(true).unwrap();
        e.set_spatial_layer_gain_db(1.).unwrap();
        e.spatial_layer.refresh_beds(&mut e.sources,&e.vbap);
        for label in ["FrontLeft","FrontRight","LFE"] {assert!(e.sources[label].spatial_layer_bed_weight<1e-6);}
        assert!((e.sources["SurroundLeft"].spatial_layer_bed_weight-e.sources["SurroundRight"].spatial_layer_bed_weight).abs()<1e-6);
        assert!(e.sources["SurroundLeft"].spatial_layer_bed_weight>0.99);
        let mut lfe=Source{lfe_gain:1.,..Default::default()};
        assert_eq!(e.spatial_layer.source_gain(&mut lfe,0),1.);
        e.set_spatial_enhancement(false).unwrap();
        e.set_spatial_layer_gain_db(4.).unwrap();
        e.spatial_layer.refresh_beds(&mut e.sources,&e.vbap);
        assert!(e.sources["SurroundLeft"].spatial_layer_bed_weight<0.1,"manual protected-side control lost its geometry");
    }
}

#[test]
fn selected_base_repeated_toggles_do_not_accumulate_gain_or_transport_changes() {
    let mut e=Engine::new(48000,2);e.sample_pos=123456;e.output_active=true;
    for _ in 0..4 {
        e.set_spatial_enhancement(true).unwrap();
        assert_eq!(e.sample_pos,123456);assert_eq!(e.spatial_preamp.target_db(),spatial_layer::MAIN_GAIN_DB);
        assert_eq!(e.spatial_layer.db,spatial_layer::EXTRA_AUXILIARY_GAIN_DB);assert_eq!(e.spatial_layer.rear_db,0.);
        e.reset_session(123456);
        assert_eq!(e.spatial_preamp.gain(),1.);
        e.set_spatial_enhancement(false).unwrap();e.reset_session(123456);
        assert_eq!(e.spatial_preamp.gain(),1.);assert_eq!(e.spatial_layer.db,0.);
    }
}
