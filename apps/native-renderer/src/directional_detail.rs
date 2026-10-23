//! Source-local spectral contrast for weak rear/height material. It trades
//! energy inside the existing auxiliary signal, rather than lifting the whole
//! layer and paying for it with common-output attenuation. No voice extraction,
//! song identity, head pose, file scan, audio cache, allocation or latency.
use crate::dsp::Biquad;

#[derive(Default)]
pub(crate) struct SourceState {
    rate: u32,
    body: Option<Biquad>,
    detail: Option<Biquad>,
    // E[high²], E[low*high], E[shape²], E[low*shape].
    energy: [f64; 4],
    alpha: f64,
    normalization: f64,
    normalization_alpha: f64,
    amount: f32,
}
impl SourceState {
    fn configure(&mut self, rate: u32) {
        *self = Self {
            rate,
            body: Biquad::butterworth_highpass(rate, 250f32.min(rate as f32*0.1)).ok(),
            detail: Biquad::butterworth_highpass(rate, 1800f32.min(rate as f32*0.2)).ok(),
            alpha: 1. - (-1./(f64::from(rate.max(1))*0.05)).exp(),
            normalization: 1.,
            normalization_alpha: 1. - (-1./(f64::from(rate.max(1))*0.02)).exp(),
            ..Default::default()
        };
    }
    pub fn process(&mut self, sample: f32, membership: f32, strength: f32, rate: u32) -> f32 {
        if self.rate != rate { self.configure(rate); }
        let x = if sample.is_finite() { sample } else { 0. };
        let high = self.body.as_mut().map_or(0., |f| f.process(x));
        let low = x-high; // Complementary: unchanged reconstruction at bypass.
        let detail = self.detail.as_mut().map_or(0., |f| f.process(high));
        let target = if membership.is_finite() && strength.is_finite() {
            membership.clamp(0.,1.) * strength.clamp(0.,1.)
        } else { 0. };
        self.amount += (target-self.amount).clamp(-1./(rate.max(1) as f32*0.03),1./(rate.max(1) as f32*0.03));
        if (self.amount-target).abs()<1e-6 { self.amount=target; }
        let shaped = high+self.amount*detail;
        let [h,l,s] = [high,low,shaped].map(f64::from);
        for (energy, value) in self.energy.iter_mut().zip([h*h,l*h,s*s,l*s]) {
            *energy += self.alpha*(value-*energy);
        }
        // Solve E[(low + k*shape)^2] = E[(low + high)^2]. The
        // complementary low component is never multiplied by k. Cross terms
        // matter near the crossover; ignoring them can silently reduce bass.
        let [hh,lh,ss,ls] = self.energy;
        let discriminant = ls*ls+ss*(hh+2.*lh);
        let norm = if ss>1e-24 && discriminant>=0. {
            let k = (-ls+discriminant.sqrt())/ss;
            if k.is_finite() && (0.25..=4.).contains(&k) { k } else { 1. }
        } else { 1. };
        self.normalization += self.normalization_alpha*(norm-self.normalization);
        if self.amount==0. { return x; }
        let out = l+self.normalization*s;
        if out.is_finite() { out as f32 } else { x }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn signal(i: usize, rate: u32) -> f32 {
        [80.,400.,3000.,8000.].iter().enumerate().map(|(j,hz)| {
            (std::f64::consts::TAU*hz*i as f64/f64::from(rate)+j as f64*0.17).sin() as f32 * [0.08,0.04,0.01,0.005][j]
        }).sum()
    }
    fn amplitude(x: &[f32], hz: f64, rate: u32) -> f64 {
        let (mut re,mut im)=(0.,0.);
        for (i,x) in x.iter().enumerate() {
            let phase=std::f64::consts::TAU*hz*i as f64/f64::from(rate);
            re+=f64::from(*x)*phase.cos();im+=f64::from(*x)*phase.sin();
        }
        (re*re+im*im).sqrt()
    }
    #[test]
    fn no_gain_recipe_or_foreground_processing_and_bypass_is_exact() {
        for rate in [44100,48000,96000] {
            let mut s=SourceState::default();
            for i in 0..rate as usize {
                let x=signal(i,rate);
                assert_eq!(s.process(x,0.,1.,rate),x);
                assert_eq!(s.process(x,1.,0.,rate),x);
            }
        }
    }
    #[test]
    fn detail_becomes_clearer_without_bass_loss_or_broadband_lift() {
        for rate in [44100,48000,96000] {
            let mut s=SourceState::default();let (mut a,mut b)=(vec![],vec![]);
            for i in 0..rate as usize*5 {
                let x=signal(i,rate);let y=s.process(x,1.,0.8,rate);
                if i>=rate as usize*4 {a.push(x);b.push(y);}
            }
            let energy=|x:&[f32]|x.iter().map(|x|f64::from(*x).powi(2)).sum::<f64>();
            let db=10.*(energy(&b)/energy(&a)).log10();
            let bass=20.*(amplitude(&b,80.,rate)/amplitude(&a,80.,rate)).log10();
            let detail=20.*(amplitude(&b,3000.,rate)/amplitude(&a,3000.,rate)).log10();
            eprintln!("rate={rate} energy={db:.4} bass={bass:.4} detail={detail:.4}");
            assert!(db.abs()<0.15,"broadband drift {db}");
            assert!(bass.abs()<0.15,"bass changed {bass}");
            assert!(detail>1.,"no useful contrast {detail}");
        }
    }
    #[test]
    fn source_splitting_and_mirrored_gain_cannot_change_the_result() {
        let (mut single,mut left,mut right)=(SourceState::default(),SourceState::default(),SourceState::default());
        let mut split:Vec<_>=(0..8).map(|_|SourceState::default()).collect();
        for i in 0..48000*2 {
            let x=signal(i,48000);let a=single.process(x,1.,0.6,48000);
            let b: f32=split.iter_mut().map(|s|s.process(x/8.,1.,0.6,48000)).sum();
            assert!((a-b).abs()<1e-6);
            let l=left.process(x,1.,0.6,48000);let r=right.process(x*0.3,1.,0.6,48000);
            assert!((l*0.3-r).abs()<2e-6,"authored pan changed");
        }
    }
    #[test]
    fn silence_rate_changes_toggle_and_128_sources_remain_finite() {
        let mut sources:Vec<_>=(0..128).map(|_|SourceState::default()).collect();
        for rate in [48000,44100,96000] {
            for i in 0..4096 {
                for s in &mut sources {
                    let x=if i<1024 {0.} else {signal(i,rate)};
                    let y=s.process(x,if i<2048{1.}else{0.},0.8,rate);
                    assert!(y.is_finite() && y.abs()<0.5);
                }
            }
        }
    }
}

#[cfg(test)]
mod renderer_tests {
    use crate::*;
    fn render(spatial:bool, fast:bool, volume:bool, head:Option<[f32;4]>) -> (Vec<f32>,f64,f32) {
        let mut e=Engine::new(48000,2);
        let manifest=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
        e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&manifest).unwrap(),0.).unwrap();
        e.set_spatial_enhancement(spatial).unwrap();e.set_program_codec("mpegh".into());
        e.set_direct_objects(true).unwrap();e.set_directional_hrtf(true);
        e.set_output_volume(1.,true);e.set_master_preamp_db(0.).unwrap();
        e.program_enabled=volume;e.set_program_target(if volume{0.7}else{1.},true);
        e.disable_fast_objects=!fast;e.head_pose=head;e.output_active=true;e.paused=false;e.direct_mix=1.;
        let frames=48000*3;
        for (i,pos) in [[0.,1.,0.],[0.,-1.,0.],[-1.,-1.,1.],[1.,-1.,1.],[0.,0.,1.]].into_iter().enumerate() {
            let mut s=Source{kind:SourceKind::Object,object_id:Some(i as u32),position:pos,gain:1.,target_gain:1.,availability:1.,availability_target:1.,..Default::default()};
            let pcm:Vec<_>=(0..frames).map(|n|{
                let phase=std::f32::consts::TAU*n as f32/48000.;
                if i==0 {0.012*(phase*997.).sin()}else{0.0004*((phase*311.).sin()+0.2*(phase*4013.).sin())}
            }).collect();
            s.samples.write(0,0,&pcm);let id=format!("generic-{i}");e.sources.insert(id.clone(),s);e.route_source_now(&id,0).unwrap();
        }
        let mut pcm=vec![0.;frames*2];for block in pcm.chunks_mut(346){e.render_into(block,2);}
        (pcm,e.output_dynamics.gain_db(),e.auxiliary_balance.directional_strength())
    }
    #[test]
    fn source_local_policy_keeps_foreground_in_both_mixers_and_volume_states() {
        for fast in [false,true] { for volume in [false,true] { for head in [None,Some([0.,0.,0.258819,0.965926])] {
            let (a,cut_a,_)=render(false,fast,volume,head);let (b,cut_b,strength)=render(true,fast,volume,head);
            assert_eq!(cut_a,0.);assert_eq!(cut_b,0.);assert!(strength>0.2,"detail inactive: fast={fast} volume={volume} head={head:?} strength={strength}");
            let tone=|pcm:&[f32],ear:usize|{
                let (mut re,mut im)=(0.,0.);for (i,frame) in pcm[48000*4..].chunks_exact(2).enumerate(){let ph=std::f64::consts::TAU*997.*i as f64/48000.;re+=f64::from(frame[ear])*ph.cos();im+=f64::from(frame[ear])*ph.sin();}(re*re+im*im).sqrt()
            };
            for ear in 0..2 {let db=20.*(tone(&b,ear)/tone(&a,ear)).log10();assert!(db.abs()<0.05,"front changed {db} dB: fast={fast} volume={volume} head={head:?}");}
            assert!(b.iter().all(|v|v.is_finite()));
        } } }
    }
}
