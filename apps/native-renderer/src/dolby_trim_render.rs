//! File-owned OAMD trims. The layout selects a DAMF surround/height tier;
//! user loudness/spatial controls do not synthesize or overwrite these values.
//! Custom gains use energy-weighted 7.1.4 reference routes so direct HRTF and
//! virtual-bus rendering have the same source gain, including reduced layouts.
use crate::{dolby_trim::{DolbyTrims,TrimUpdate}, vbap, Source, SourceKind};
use std::collections::BTreeMap;

pub(crate) struct Timeline {
    entries: BTreeMap<u64, (u64,DolbyTrims)>,
    next_revision: u64,
    reference: vbap::VbapSolver,
}
impl Default for Timeline {
    fn default()->Self {Self{entries:BTreeMap::new(),next_revision:1,reference:vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby7_1_4)}}
}
impl Timeline {
    pub fn queue(&mut self,updates:Vec<TrimUpdate>)->Result<(),String> {
        if updates.len()>128 || updates.iter().any(|u|!u.trims.valid()) {return Err("invalid Dolby downmix trim metadata".into());}
        if self.entries.len()+updates.len()>1024 {return Err("Dolby trim timeline capacity exceeded".into());}
        for update in updates {
            if self.entries.range(..=update.sample_pos).next_back().is_some_and(|(_,(_,previous))|previous==&update.trims) {continue;}
            let revision=self.next_revision;self.next_revision=self.next_revision.wrapping_add(1).max(1);
            self.entries.insert(update.sample_pos,(revision,update.trims));
        }
        Ok(())
    }
    pub fn prune(&mut self,at:u64) {
        while self.entries.keys().nth(1).is_some_and(|time|*time<=at) {self.entries.pop_first();}
    }
    fn at(&self,at:u64)->Option<(u64,&DolbyTrims)> {self.entries.range(..=at).next_back().map(|(_, (revision,trims))|(*revision,trims))}
}

pub(crate) fn layout_tiers(layout:vbap::LayoutId)->(usize,usize) {
    let speakers=vbap::speakers(layout);
    let surrounds=speakers.iter().filter(|s|s.elevation.abs()<1. && s.azimuth.abs()>45.).count();
    let heights=speakers.iter().filter(|s|s.elevation>1.).count();
    let tier=|n|if n==0 {0} else if n<=2 {1} else {2};
    (tier(surrounds),tier(heights))
}

#[derive(Clone,Copy)]
pub(crate) struct SourceTrim {
    revision:u64, layout:Option<vbap::LayoutId>, position:[f32;3], spread:f32,
    gain:f32, target:f32, step:f32, remaining:u32, initialized:bool,
}
impl Default for SourceTrim {fn default()->Self {Self{revision:0,layout:None,position:[f32::NAN;3],spread:f32::NAN,gain:1.,target:1.,step:0.,remaining:0,initialized:false}}}

fn bed_role(label:&str)->&str {
    match label {
        "L"|"Left"|"FrontLeft"=>"L", "R"|"Right"|"FrontRight"=>"R",
        "C"|"Center"|"Centre"=>"C", "LFE"|"LowFrequencyEffects"=>"LFE",
        "Lss"|"Ls"|"Lsc"|"Lsd"|"SurroundLeft"=>"Ls",
        "Rss"|"Rs"|"Rsc"|"Rsd"|"SurroundRight"=>"Rs",
        "Lrs"|"Lb"|"RearLeft"=>"Lb", "Rrs"|"Rb"|"RearRight"=>"Rb",
        "Lfh"|"Tfl"|"TopFrontLeft"=>"Tfl", "Rfh"|"Tfr"|"TopFrontRight"=>"Tfr",
        "Lts"|"Tsl"|"TopMiddleLeft"|"TopSurroundLeft"=>"Tsl",
        "Rts"|"Tsr"|"TopMiddleRight"|"TopSurroundRight"=>"Tsr",
        "Lrh"|"Tbl"|"Trl"|"TopRearLeft"=>"Tbl", "Rrh"|"Tbr"|"Trr"|"TopRearRight"=>"Tbr",
        other=>other,
    }
}

fn disabled(trims:&DolbyTrims,source:&Source)->bool {
    source.object_id.is_some_and(|id|trims.disabled_objects.contains(&id))
        || source.bed_label.as_ref().is_some_and(|label|trims.disabled_beds.iter().any(|s|bed_role(s)==bed_role(label)))
}

fn target(timeline:&Timeline,trims:&DolbyTrims,source:&Source,layout:vbap::LayoutId)->f32 {
    if disabled(trims,source) || source.lfe_gain!=0. || source.lfe_target!=0. {return 1.;}
    let tiers=layout_tiers(layout);
    let Some(c)=trims.configuration(tiers.0,tiers.1) else {return 1.;};
    let route=if source.kind==SourceKind::Bed {crate::bed_route(source.bed_label.as_deref().unwrap_or(""),&timeline.reference).buses}
        else {timeline.reference.pan(source.render_position(),source.spread)};
    let mut input_power=0.;let mut output_power=0.;
    for (gain,speaker) in route.iter().zip(vbap::speakers(vbap::LayoutId::Dolby7_1_4)) {
        let power=gain*gain;
        let db=if speaker.elevation>1. {c.height_db.unwrap_or(0.)}
            else if speaker.name=="Center" {c.centre_db.unwrap_or(0.)}
            else if speaker.azimuth.abs()>45. {c.surround_db.unwrap_or(0.)} else {0.};
        // Front/back balance is an authored attenuation balance, not a new
        // loudness target. Positive metadata moves balance toward the back.
        let balance=if speaker.elevation.abs()>1. {c.top_bottom_y.unwrap_or(0.)} else {c.listener_y.unwrap_or(0.)};
        let front=(speaker.azimuth.to_radians().cos()+1.)*0.5;
        let balance_power=1.-balance.max(0.)*front-(-balance).max(0.)*(1.-front);
        input_power+=power;output_power+=power*10f32.powf(db/10.)*balance_power.max(0.);
    }
    if input_power>1e-12 {(output_power/input_power).max(0.).sqrt()} else {1.}
}

/// Warp is a file-owned render transform; never modify decoder positions,
/// interpolation targets, object IDs, or distances in metres.
pub(crate) fn update_warp(timeline:&Timeline,source:&mut Source,at:u64)->bool {
    let scale=if source.kind==SourceKind::Object && timeline.at(at).is_some_and(|(_,t)|t.warp_mode==1) {2.} else {1.};
    if source.dolby_y_scale==scale {false} else {source.dolby_y_scale=scale;true}
}

pub(crate) fn source_gain(timeline:&Timeline,source:&mut Source,layout:vbap::LayoutId,at:u64)->f32 {
    if timeline.entries.is_empty() && !source.dolby_trim.initialized {return 1.;}
    let state=timeline.at(at);
    let revision=state.map_or(0,|s|s.0);
    let refresh=!source.dolby_trim.initialized || source.dolby_trim.revision!=revision
        || source.dolby_trim.layout!=Some(layout)
        || (at%128==0 && (source.position!=source.dolby_trim.position || source.spread!=source.dolby_trim.spread));
    if refresh {
        let goal=state.map_or(1.,|(_,trims)|target(timeline,trims,source,layout));
        let s=&mut source.dolby_trim;
        if !s.initialized {s.gain=goal;s.remaining=0;} else if goal!=s.target {s.remaining=128;s.step=(goal-s.gain)/128.;}
        s.target=goal;s.revision=revision;s.layout=Some(layout);s.position=source.position;s.spread=source.spread;s.initialized=true;
    }
    let s=&mut source.dolby_trim;
    let result=s.gain;
    if s.remaining>0 {s.gain+=s.step;s.remaining-=1;if s.remaining==0 {s.gain=s.target;}}
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dolby_trim::TrimConfiguration;
    fn metadata()->DolbyTrims {
        DolbyTrims{global_mode:2,configurations:vec![TrimConfiguration{mode:2,centre_db:Some(-3.),surround_db:Some(-6.),height_db:Some(-9.),..Default::default()};9],..Default::default()}
    }
    #[test]
    fn actual_speaker_layout_selects_surround_and_height_tiers() {
        for (name,expected) in [("2.0",(0,0)),("5.1",(1,0)),("7.1",(2,0)),("5.1.2",(1,1)),("7.1.4",(2,2)),("9.1.6",(2,2))] {
            assert_eq!(layout_tiers(vbap::LayoutId::parse(name).unwrap()),expected,"{name}");
        }
    }
    #[test]
    fn no_metadata_is_neutral_and_invalid_batches_are_atomic() {
        let mut timeline=Timeline::default();assert!(timeline.at(0).is_none());
        assert!(timeline.queue(vec![TrimUpdate{sample_pos:0,trims:metadata()},TrimUpdate{sample_pos:1,trims:DolbyTrims{global_mode:3,..Default::default()}}]).is_err());
        assert!(timeline.at(10).is_none());
    }
    #[test]
    fn timeline_is_sample_addressed_and_pruning_keeps_current_state() {
        let mut t=Timeline::default();t.queue(vec![TrimUpdate{sample_pos:100,trims:metadata()},TrimUpdate{sample_pos:200,trims:DolbyTrims{global_mode:1,..Default::default()}}]).unwrap();
        assert!(t.at(99).is_none());assert_eq!(t.at(100).unwrap().1.global_mode,2);assert_eq!(t.at(200).unwrap().1.global_mode,1);
        t.prune(250);assert_eq!(t.entries.len(),1);assert_eq!(t.at(251).unwrap().1.global_mode,1);
    }
    fn object(position:[f32;3])->Source {Source{kind:SourceKind::Object,object_id:Some(42),position,gain:0.2,target_gain:0.2,..Default::default()}}
    #[test]
    fn dolby_custom_trims_act_on_centre_surround_and_height_sources_without_changing_codec_gain() {
        let mut timeline=Timeline::default();timeline.queue(vec![TrimUpdate{sample_pos:0,trims:metadata()}]).unwrap();
        for (position,db) in [([0.,1.,0.],-3.),([0.,-1.,0.],-6.),([0.,0.,1.],-9.)] {
            let mut source=object(position);let gain=source_gain(&timeline,&mut source,vbap::LayoutId::Dolby7_1_4,0);
            assert!((20.*gain.log10()-db).abs()<0.001,"position={position:?}, got {} dB",20.*gain.log10());
            assert_eq!(source.gain,0.2);assert_eq!(source.position,position);
        }
    }
    #[test]
    fn dolby_disable_flags_and_lfe_bypass_do_not_remove_or_mute_any_source() {
        let mut m=metadata();m.disabled_objects=vec![42];m.disabled_beds=vec!["Center".into()];
        let mut timeline=Timeline::default();timeline.queue(vec![TrimUpdate{sample_pos:0,trims:m}]).unwrap();
        let mut source=object([0.,-1.,0.]);assert_eq!(source_gain(&timeline,&mut source,vbap::LayoutId::Stereo2_0,0),1.);assert_eq!(source.object_id,Some(42));
        source=Source{kind:SourceKind::Bed,bed_label:Some("Center".into()),..Default::default()};assert_eq!(source_gain(&timeline,&mut source,vbap::LayoutId::Dolby5_1,0),1.);
        source=object([0.,0.,1.]);source.object_id=Some(41);source.lfe_target=1.;assert_eq!(source_gain(&timeline,&mut source,vbap::LayoutId::Dolby7_1_4,0),1.);
    }
    #[test]
    fn dolby_trim_updates_start_on_codec_sample_and_restore_neutrally() {
        let mut timeline=Timeline::default();timeline.queue(vec![TrimUpdate{sample_pos:32,trims:metadata()},TrimUpdate{sample_pos:256,trims:DolbyTrims{global_mode:1,..Default::default()}}]).unwrap();
        let mut s=object([0.,-1.,0.]);for at in 0..=32 {assert_eq!(source_gain(&timeline,&mut s,vbap::LayoutId::Dolby7_1_4,at),1.);}
        for at in 33..256 {let g=source_gain(&timeline,&mut s,vbap::LayoutId::Dolby7_1_4,at);assert!(g.is_finite()&&g<=1.&&g>0.);}
        assert!((s.dolby_trim.gain-10f32.powf(-6./20.)).abs()<1e-6);
        for at in 256..=384 {source_gain(&timeline,&mut s,vbap::LayoutId::Dolby7_1_4,at);}assert_eq!(s.dolby_trim.gain,1.);
    }
    #[test]
    fn dolby_warp_changes_only_render_coordinates_and_is_timed() {
        let mut t=Timeline::default();t.queue(vec![TrimUpdate{sample_pos:100,trims:DolbyTrims{warp_mode:1,..Default::default()}}]).unwrap();
        let mut s=object([0.4,0.7,0.2]);assert!(!update_warp(&t,&mut s,99));assert!(update_warp(&t,&mut s,100));
        assert_eq!(s.position,[0.4,0.7,0.2]);assert_eq!(s.render_position(),[0.4,1.4,0.2]);
        assert!(!update_warp(&t,&mut s,101));
    }

    fn render_trimmed_pcm(trims:Option<DolbyTrims>,direct:bool,fast:bool)->Vec<f32> {
        render_trimmed_pcm_at(trims,direct,fast,[0.,-1.,0.])
    }
    fn render_trimmed_pcm_at(trims:Option<DolbyTrims>,direct:bool,fast:bool,position:[f32;3])->Vec<f32> {
        let hrtf=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
        let mut e=crate::Engine::new(48_000,2);
        e.replace_hrtf(crate::hrtf::NativeHrtfSet::load_calibrated(&hrtf).unwrap(),0.).unwrap();
        e.set_spatial_enhancement(false).unwrap();e.set_direct_objects(direct).unwrap();e.set_directional_hrtf(true);
        e.disable_fast_objects=!fast;e.direct_mix=if direct {1.} else {0.};e.program_codec=Some("eac3".into());
        e.output_active=true;e.paused=false;
        if let Some(trims)=trims {e.downmix_trims.queue(vec![TrimUpdate{sample_pos:0,trims}]).unwrap();}
        let mut source=object(position);source.gain=1.;source.target_gain=1.;source.availability=1.;source.availability_target=1.;
        let pcm=(0..8192).map(|i|(i as f32*0.11).sin()*0.001).collect::<Vec<_>>();source.samples.write(0,0,&pcm);
        e.sources.insert("obj:42".into(),source);e.route_source_now("obj:42",0).unwrap();
        // The source-major mixer intentionally starts at eight objects. Add
        // seven silent synthetic lanes so this test actually exercises it.
        if direct && fast {
            for index in 0..7 {
                let id=100+index;let mut silent=object(position);silent.object_id=Some(id);silent.gain=1.;silent.target_gain=1.;
                silent.availability=1.;silent.availability_target=1.;silent.samples.write(0,0,&vec![0.;8192]);
                let key=format!("obj:{id}");e.sources.insert(key.clone(),silent);e.route_source_now(&key,0).unwrap();
            }
        }
        let mut output=vec![0.;16384];
        for block in output.chunks_mut(1024) {e.render_into(block,2);}
        if direct && fast { assert!(e.fast_object_blocks>0,"continuous object mixer was not exercised"); }
        assert_eq!(e.sources.len(),if direct && fast {8} else {1});assert_eq!(e.sources["obj:42"].object_id,Some(42));assert_eq!(e.sources["obj:42"].position,position);
        assert!(output.iter().all(|x|x.is_finite()));assert!(output.iter().any(|x|x.abs()>1e-7));
        assert_eq!(e.peak_guard.diagnostic_gain(),1.);
        output
    }
    #[test]
    fn dolby_trim_scales_actual_pcm_in_both_mixers_and_default_is_bit_exact() {
        for direct in [false,true] {for fast in [false,true] {
            let original=render_trimmed_pcm(None,direct,fast);
            let defaults=render_trimmed_pcm(Some(DolbyTrims::default()),direct,fast);
            assert_eq!(original,defaults,"default trim changed PCM: direct={direct} fast={fast}");
            let custom=render_trimmed_pcm(Some(metadata()),direct,fast);let gain=10f32.powf(-6./20.);
            let error=original.iter().zip(&custom).map(|(a,b)|(a*gain-b).abs()).fold(0.,f32::max);
            assert!(error<2e-6,"PCM did not follow -6 dB trim: direct={direct} fast={fast}, error={error}");
        }}
    }

    #[test]
    fn dolby_trims_survive_renderer_path_changes_and_clear_only_with_the_programme() {
        let mut e=crate::Engine::new(48000,2);e.downmix_trims.queue(vec![TrimUpdate{sample_pos:0,trims:metadata()}]).unwrap();
        e.reset_object_renderers();assert!(e.downmix_trims.at(0).is_some(),"HRTF changes must not discard file metadata");
        e.reset_session(9000);assert!(e.downmix_trims.at(9000).is_none(),"new programme must not inherit trims");
    }

    #[test]
    fn dolby_warp_pcm_matches_the_equivalent_authored_direction_without_rewriting_metadata() {
        let trims=DolbyTrims{warp_mode:1,..Default::default()};
        for fast in [false,true] {
            let warped=render_trimmed_pcm_at(Some(trims.clone()),true,fast,[0.4,0.7,0.2]);
            let expected=render_trimmed_pcm_at(None,true,fast,[0.4,1.4,0.2]);
            let error=warped.iter().zip(expected).map(|(a,b)|(a-b).abs()).fold(0.,f32::max);
            assert!(error<1e-7,"warp changed PCM inconsistently: {error}, fast={fast}");
        }
    }

    #[test]
    fn dolby_truehd_bed_disable_roles_match_pcm_labels_without_losing_metadata_spelling() {
        let mut m=metadata();m.disabled_beds=vec!["Lss".into(),"Lfh".into(),"Lrh".into(),"C".into()];
        let mut t=Timeline::default();t.queue(vec![TrimUpdate{sample_pos:0,trims:m}]).unwrap();
        for label in ["SurroundLeft","Tfl","TopRearLeft","Center"] {
            let mut s=Source{kind:SourceKind::Bed,bed_label:Some(label.into()),..Default::default()};
            assert_eq!(source_gain(&t,&mut s,vbap::LayoutId::Dolby7_1_4,0),1.,"disable flag missed {label}");
        }
    }

}
