//! Source-local spectral contrast for weak rear/height material. It trades
//! energy inside the existing auxiliary signal, rather than lifting the whole
//! layer and paying for it with common-output attenuation. No voice extraction,
//! song identity, head pose, file scan, audio cache, allocation or latency.
use crate::dsp::Biquad;

// Preserve the source's body rather than spending it on a treble enhancement.
// 0.45 dB leaves a small margin below the 0.5 dB regression limit between
// sampled frequencies. This is a source-local spectral floor, not output gain.
const BODY_FLOOR: f64 = 0.9495109992021984;

#[derive(Default)]
struct BodyProtection { real: [[f64;2];3] }
impl BodyProtection {
    fn new(high:&Biquad, detail:&Biquad, air:&Biquad, rate:u32)->Self {
        let mut real=[[f64::INFINITY,f64::NEG_INFINITY];3];
        let mul=|a:[f64;2],b:[f64;2]|[a[0]*b[0]-a[1]*b[1],a[0]*b[1]+a[1]*b[0]];
        for step in 0..=256 {
            let frequency=80.+(2000f64.min(f64::from(rate)*0.4)-80.)*step as f64/256.;
            let h=high.frequency_response(rate,frequency);
            let d=mul(h,detail.frequency_response(rate,frequency));
            let a=mul(d,air.frequency_response(rate,frequency));
            for (i,v) in [h[0],d[0],a[0]].iter().enumerate() {real[i][0]=real[i][0].min(*v);real[i][1]=real[i][1].max(*v);}
        } Self {real}
    }
    fn mix(&self,k:f64,amount:[f32;2])->f64 {
        let coefficients=[k-1.,k*f64::from(amount[0]),k*f64::from(amount[1]-amount[0])];
        let lower=coefficients.iter().zip(self.real).map(|(c,b)|c*if *c<0.{b[1]}else{b[0]}).sum::<f64>();
        if lower<BODY_FLOOR-1. {(1.-BODY_FLOOR)/-lower}else{1.}
    }
}

#[derive(Default)]
pub(crate) struct SourceState {
    rate: u32,
    body: Option<Biquad>,
    detail: Option<Biquad>,
    air: Option<Biquad>,
    fast: f64,
    slow: f64,
    permission: f32,
    fast_alpha: f64,
    slow_alpha: f64,
    // E[high²], E[low*high], E[shape²], E[low*shape].
    energy: [f64; 4],
    alpha: f64,
    normalization: f64,
    normalization_alpha: f64,
    amount: [f32;2],
    body_protection: BodyProtection,
}
impl SourceState {
    fn configure(&mut self, rate: u32) {
        let body = Biquad::butterworth_highpass(rate, 250f32.min(rate as f32 * 0.1)).ok();
        let detail = Biquad::butterworth_highpass(rate, 1800f32.min(rate as f32 * 0.2)).ok();
        let air = Biquad::butterworth_highpass(rate, 5000f32.min(rate as f32 * 0.3)).ok();
        let body_protection = match (&body, &detail, &air) {
            (Some(h), Some(d), Some(a)) => BodyProtection::new(h, d, a, rate),
            _ => BodyProtection::default(),
        };
        *self = Self {
            rate,
            body,
            detail, air, permission: 1.,
            fast_alpha: 1.-(-1./(f64::from(rate.max(1))*0.003)).exp(),
            slow_alpha: 1.-(-1./(f64::from(rate.max(1))*0.080)).exp(),
            body_protection,
            alpha: 1. - (-1. / (f64::from(rate.max(1)) * 0.05)).exp(),
            normalization: 1.,
            normalization_alpha: 1. - (-1. / (f64::from(rate.max(1)) * 0.02)).exp(),
            ..Default::default()
        };
    }
    #[cfg(test)]
    pub fn process(&mut self, sample: f32, membership: f32, strength: f32, rate: u32) -> f32 { self.process_bands(sample,membership,[strength;2],rate) }
    pub fn envelope_permission(&self)->f32 { self.permission }
    pub fn process_bands(&mut self, sample: f32, membership: f32, strength: [f32;2], rate: u32) -> f32 {
        if self.rate != rate {
            self.configure(rate);
        }
        let x = if sample.is_finite() { sample } else { 0. };
        let high = self.body.as_mut().map_or(0., |f| f.process(x));
        let low = x - high; // Complementary: unchanged reconstruction at bypass.
        let detail = self.detail.as_mut().map_or(0., |f| f.process(high));
        let air = self.air.as_mut().map_or(0., |f| f.process(detail));
        let power=f64::from(x).powi(2);
        self.fast+=self.fast_alpha*(power-self.fast);self.slow+=self.slow_alpha*(power-self.slow);
        let envelope_target=if self.fast>self.slow*3. || (self.slow>1e-12 && self.fast<self.slow*0.2) {0.25}else{1.};
        let seconds=if envelope_target<self.permission {0.002}else{0.10};
        self.permission+=(envelope_target-self.permission)*(1.-(-1./(rate.max(1) as f32*seconds)).exp());
        for band in 0..2 {
            let target=if membership.is_finite() && strength[band].is_finite() {membership.clamp(0.,1.)*strength[band].clamp(0.,1.)*self.permission}else{0.};
            self.amount[band]+=(target-self.amount[band]).clamp(-1./(rate.max(1) as f32*0.03),1./(rate.max(1) as f32*0.03));
            if (self.amount[band]-target).abs()<1e-6 {self.amount[band]=target;}
        }
        let shaped=high+self.amount[0]*(detail-air)+self.amount[1]*air;
        let [h, l, s] = [high, low, shaped].map(f64::from);
        for (energy, value) in self.energy.iter_mut().zip([h * h, l * h, s * s, l * s]) {
            *energy += self.alpha * (value - *energy);
        }
        // Solve E[(low + k*shape)^2] = E[(low + high)^2]. The
        // complementary low component is never multiplied by k. Cross terms
        // matter near the crossover; ignoring them can silently reduce bass.
        let [hh, lh, ss, ls] = self.energy;
        let discriminant = ls * ls + ss * (hh + 2. * lh);
        let norm = if ss > 1e-24 && discriminant >= 0. {
            let k = (-ls + discriminant.sqrt()) / ss;
            if k.is_finite() && (0.25..=4.).contains(&k) {
                k
            } else {
                1.
            }
        } else {
            1.
        };
        self.normalization += self.normalization_alpha * (norm - self.normalization);
        if self.amount == [0.;2] {
            return x;
        }
        let enhanced = l + self.normalization * s;
        let mix = self
            .body_protection
            .mix(self.normalization, self.amount);
        let out = f64::from(x) + mix * (enhanced - f64::from(x));
        if out.is_finite() {
            out as f32
        } else {
            x
        }
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
            // The body floor withdraws large boosts that would thin the midrange.
            assert!(detail>0.2,"no useful contrast {detail}");
        }
    }
    #[test]
    fn rear_body_is_preserved_for_bass_balanced_and_bright_sources() {
        let frequencies = [80., 250., 500., 1000., 2000., 3000., 6000., 10000.];
        let shapes = [
            [1.; 8],
            [1., 0.8, 0.4, 0.2, 0.1, 0.04, 0.015, 0.01],
            [0.1, 0.15, 0.2, 0.3, 0.5, 0.8, 1., 1.],
        ];
        for rate in [44100, 48000, 96000] {
            for (shape, levels) in shapes.iter().enumerate() {
                let mut state = SourceState::default();
                let (mut input, mut output) = (Vec::new(), Vec::new());
                for i in 0..rate as usize * 2 {
                    let x = frequencies
                        .iter()
                        .zip(levels)
                        .enumerate()
                        .map(|(j, (hz, level))| {
                            (std::f64::consts::TAU * hz * i as f64 / f64::from(rate)
                                + j as f64 * 0.17)
                                .sin() as f32
                                * *level
                                * 0.004
                        })
                        .sum();
                    let y = state.process(x, 1., 0.8, rate);
                    if i >= rate as usize {
                        input.push(x);
                        output.push(y);
                    }
                }
                let energy =
                    |samples: &[f32]| samples.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
                let energy_db = 10. * (energy(&output) / energy(&input)).log10();
                assert!(
                    energy_db.abs() < 0.15,
                    "source energy drift {energy_db}dB: rate={rate} shape={shape}"
                );
                for frequency in [80., 250., 500., 1000., 2000.] {
                    let db = 20.
                        * (amplitude(&output, frequency, rate)
                            / amplitude(&input, frequency, rate))
                        .log10();
                    eprintln!("body rate={rate} shape={shape} hz={frequency} db={db:.4} energy={energy_db:.4}");
                    assert!(
                        (-0.5..=0.5).contains(&db),
                        "body changed {db}dB at {frequency}Hz: rate={rate} shape={shape}"
                    );
                }
                let detail_db = 20.
                    * (amplitude(&output, 6000., rate) / amplitude(&input, 6000., rate)).log10();
                let body_db = 20.
                    * (amplitude(&output, 1000., rate) / amplitude(&input, 1000., rate)).log10();
                assert!(detail_db-body_db>0.15,"lost directional contrast: detail={detail_db} body={body_db}: rate={rate} shape={shape}");
            }
        }
    }

    #[test]
    fn protection_covers_the_band_between_its_configuration_points() {
        for rate in [44100, 48000, 96000] {
            let high = Biquad::butterworth_highpass(rate, 250.).unwrap();
            let detail = Biquad::butterworth_highpass(rate, 1800.).unwrap();
            let air=Biquad::butterworth_highpass(rate,5000.).unwrap();
            let protection = BodyProtection::new(&high, &detail, &air, rate);
            for norm in [0.25, 0.5, 0.75, 1., 1.25, 2., 4.] {
                for amount in [0., 0.1, 0.6, 1.] {
                    let mix = protection.mix(norm, [amount as f32;2]);
                    assert!((0.0..=1.0).contains(&mix));
                    for frequency in (80..=2000).step_by(3) {
                        let h = high.frequency_response(rate, frequency as f64);
                        let d = detail.frequency_response(rate, frequency as f64);
                        let re = 1.
                            + mix
                                * ((norm - 1.) * h[0]
                                    + norm * amount * (h[0] * d[0] - h[1] * d[1]));
                        let im = mix
                            * ((norm - 1.) * h[1] + norm * amount * (h[0] * d[1] + h[1] * d[0]));
                        let db = 10. * (re * re + im * im).log10();
                        assert!(db>=-0.5,"body protection missed {frequency}Hz: {db}dB rate={rate} k={norm} amount={amount}");
                    }
                }
            }
        }
    }

    #[test]
    fn active_to_bypass_restores_samples_and_invalid_controls_stay_finite() {
        for rate in [44100, 48000, 96000] {
            let mut state = SourceState::default();
            for i in 0..rate as usize {
                let enabled = i < rate as usize / 2;
                let x = signal(i, rate);
                let y = state.process(x, 1., if enabled { 0.8 } else { 0. }, rate);
                assert!(y.is_finite());
                if i > rate as usize * 3 / 4 {
                    assert_eq!(y, x, "bypass retained processing");
                }
            }
            for x in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.] {
                assert!(state.process(x, f32::NAN, f32::INFINITY, rate).is_finite());
            }
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

#[cfg(test)]
mod perceptual_regression_tests {
    use super::*;
    #[test]
    fn onset_and_decay_only_withdraw_added_enhancement() {
        for rate in [44100,48000,96000] {
            let mut s=SourceState::default();let mut onset=1f32;let mut tail=1f32;
            for i in 0..rate as usize {
                let t=i as f32/rate as f32;
                let envelope=if t<0.1{0.}else{(-(t-0.1)/0.08).exp()};
                let x=(std::f32::consts::TAU*3000.*t).sin()*0.1*envelope;
                let y=s.process_bands(x,1.,[0.8,0.4],rate);assert!(y.is_finite());
                if (0.105..0.12).contains(&t){onset=onset.min(s.envelope_permission());}
                if (0.4..0.5).contains(&t){tail=tail.min(s.envelope_permission());}
            }
            assert!(onset<0.6,"unprotected onset: {rate} {onset}");
            assert!(tail<0.5,"tail is chasing gain: {rate} {tail}");
        }
    }
    #[test]
    fn selective_bands_remain_energy_bounded_and_keep_body() {
        for bands in [[0.8,0.],[0.,0.8],[0.8,0.8]] {for rate in [44100,48000,96000] {
            let mut state=SourceState::default();let (mut input,mut output)=(0.,0.);
            let (mut bass_in,mut bass_out)=(0.,0.);
            for i in 0..rate as usize*2 {
                let phase=std::f64::consts::TAU*i as f64/f64::from(rate);
                let x=(0.04*(phase*150.).sin()+0.015*(phase*3000.).sin()+0.01*(phase*8000.).sin())as f32;
                let y=state.process_bands(x,1.,bands,rate);assert!(y.is_finite());
                if i>=rate as usize {input+=f64::from(x).powi(2);output+=f64::from(y).powi(2);bass_in+=f64::from(x)*(phase*150.).sin();bass_out+=f64::from(y)*(phase*150.).sin();}
            }
            let energy=10.*(output/input).log10();let body=20.*(bass_out/bass_in).abs().log10();
            assert!(energy.abs()<0.5,"rate={rate} bands={bands:?} energy={energy}");
            assert!(body> -0.5,"rate={rate} bands={bands:?} body={body}");
        }}
    }
}
