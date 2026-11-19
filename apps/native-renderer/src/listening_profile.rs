//! Bounded personal listening preferences, not measured ear/headphone correction.
//! Both ears share broad shelves before final loudness/peak measurement. No HRTF
//! notch equalisation, delay changes or independent ear gains are introduced.
use crate::dsp::Biquad;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub rear: f32,
    pub height: f32,
    pub bass_db: f32,
    pub treble_db: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            rear: 1.,
            height: 1.,
            bass_db: 0.,
            treble_db: 0.,
        }
    }
}
impl Settings {
    pub fn validate(self) -> Result<Self, String> {
        if !self.rear.is_finite()
            || !(0.5..=1.5).contains(&self.rear)
            || !self.height.is_finite()
            || !(0.5..=1.5).contains(&self.height)
            || !self.bass_db.is_finite()
            || !(-3.0..=3.0).contains(&self.bass_db)
            || !self.treble_db.is_finite()
            || !(-3.0..=3.0).contains(&self.treble_db)
        {
            return Err("试听参数超出范围".into());
        }
        Ok(self)
    }
    pub(crate) fn directional(self, position: [f32; 3]) -> f32 {
        let [x, y, z] = position;
        let r = (x * x + y * y + z * z).sqrt();
        if !r.is_finite() || r < 1e-8 {
            return 1.;
        }
        let rear = (-y / r).max(0.);
        let vertical = (z / r).abs();
        let sum = rear + vertical;
        // Convex weights: no double dose at upper/lower rear intersections.
        1. + (rear * (self.rear - 1.) + vertical * (self.height - 1.)) / sum.max(1.)
    }
}
pub(crate) struct Processor {
    settings: Settings,
    bank: [[Biquad; 2]; 2],
    target: [[Biquad; 2]; 2],
    mix: f32,
    rate: u32,
    pending: Option<Settings>,
}
impl Processor {
    pub fn new(rate: u32) -> Self {
        let bank = Self::filters(rate, Settings::default());
        Self {
            settings: Settings::default(),
            bank,
            target: bank,
            mix: 1.,
            rate,
            pending: None,
        }
    }
    fn filters(rate: u32, s: Settings) -> [[Biquad; 2]; 2] {
        [[
            Biquad::lowshelf(
                rate,
                120f32.min(rate as f32 * 0.1),
                std::f32::consts::FRAC_1_SQRT_2,
                s.bass_db,
            )
            .unwrap(),
            Biquad::highshelf(
                rate,
                6000f32.min(rate as f32 * 0.3),
                std::f32::consts::FRAC_1_SQRT_2,
                s.treble_db,
            )
            .unwrap(),
        ]; 2]
    }
    pub fn set(&mut self, s: Settings) -> Result<(), String> {
        let s = s.validate()?;
        // Coalesce quick UI changes; never reset an audible crossfade midway.
        if self.mix < 1. {
            self.pending = Some(s);
            return Ok(());
        }
        self.settings = s;
        self.target = Self::filters(self.rate, s);
        self.mix = 0.;
        Ok(())
    }
    pub fn reset(&mut self) {
        let s = self.pending.take().unwrap_or(self.settings);
        *self = Self::new(self.rate);
        self.settings = s;
        self.bank = Self::filters(self.rate, s);
        self.target = self.bank;
    }
    pub fn process(&mut self, x: [f32; 2]) -> [f32; 2] {
        let run = |bank: &mut [[Biquad; 2]; 2]| {
            std::array::from_fn(|i| {
                let a = bank[i][0].process(x[i]);
                bank[i][1].process(a)
            })
        };
        let old = run(&mut self.bank);
        if self.mix >= 1. {
            return old;
        }
        let new = run(&mut self.target);
        let m = self.mix;
        self.mix = (self.mix + 1. / (self.rate.max(1) as f32 * 0.08)).min(1.);
        if self.mix == 1. {
            self.bank = self.target;
            if let Some(s) = self.pending.take() {
                let _ = self.set(s);
            }
        }
        std::array::from_fn(|i| old[i] * (1. - m) + new[i] * m)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_are_bounded_atomic_and_defaults_are_bit_exact() {
        let mut p = Processor::new(48000);
        for i in 0..10000 {
            let x = [(i as f32 * 0.1).sin() * 0.1, -0.02];
            assert_eq!(p.process(x), x);
        }
        assert!(p
            .set(Settings {
                bass_db: f32::NAN,
                ..Default::default()
            })
            .is_err());
        for key in ["rear", "height", "bassDb", "trebleDb"] {
            let s = format!("{{\"{key}\":99}}");
            assert!(serde_json::from_str::<Settings>(&s)
                .unwrap()
                .validate()
                .is_err());
        }
    }
    #[test]
    fn corner_weights_are_continuous_and_do_not_stack() {
        let s = Settings {
            rear: 1.5,
            height: 1.5,
            ..Default::default()
        };
        for y in -100..=100 {
            for z in -100..=100 {
                let v = s.directional([0.1, y as f32 / 100., z as f32 / 100.]);
                assert!((1.0..=1.500001).contains(&v));
            }
        }
        assert_eq!(s.directional([0., 1., 0.]), 1.);
    }
    #[test]
    fn quick_changes_remain_linked_and_reset_keeps_last_preference() {
        let mut p = Processor::new(48000);
        for i in 0..20000 {
            if i % 250 == 0 {
                p.set(Settings {
                    bass_db: if i % 500 == 0 { 3. } else { -3. },
                    ..Default::default()
                })
                .unwrap();
            }
            let y = p.process([0.1, 0.1]);
            assert_eq!(y[0], y[1]);
            assert!(y[0].is_finite() && y[0].abs() < 0.3);
        }
        let last = p.pending.unwrap_or(p.settings);
        p.reset();
        assert_eq!(p.settings, last);
    }
}

#[cfg(test)]
mod audio_response_tests {
    use super::*;
    #[test]
    fn shelves_have_real_bounded_response_without_changing_ear_difference() {
        for rate in [44100, 48000, 96000] {
            for gain in [-3., 3.] {
                let mut p = Processor::new(rate);
                p.set(Settings {
                    bass_db: gain,
                    treble_db: gain,
                    ..Default::default()
                })
                .unwrap();
                let (mut a, mut b) = (0., 0.);
                for i in 0..rate * 2 {
                    let x = 0.04 * (std::f32::consts::TAU * 40. * i as f32 / rate as f32).sin();
                    let y = p.process([x, x * 0.5]);
                    assert!((y[0] * 0.5 - y[1]).abs() < 1e-7);
                    if i >= rate {
                        a += f64::from(x).powi(2);
                        b += f64::from(y[0]).powi(2);
                    }
                }
                let measured = 10. * (b / a).log10();
                assert!(
                    (measured - f64::from(gain)).abs() < 0.25,
                    "{rate} requested={gain} measured={measured}"
                );
            }
        }
    }
    #[test]
    fn one_degree_rear_top_bottom_grid_has_no_calibration_boundary() {
        let s = Settings {
            rear: 1.5,
            height: 0.5,
            ..Default::default()
        };
        for az in -180..=180 {
            let mut previous: Option<f32> = None;
            for el in -90..=90 {
                let (a, e) = ((az as f32).to_radians(), (el as f32).to_radians());
                let p = [a.sin() * e.cos(), a.cos() * e.cos(), e.sin()];
                let v = s.directional(p);
                if let Some(old) = previous {
                    assert!((v - old).abs() < 0.02, "{az}/{el}");
                }
                previous = Some(v);
            }
        }
    }
}

#[cfg(test)]
mod renderer_tests {
    use crate::*;
    fn render(gain: f32, fast: bool) -> Vec<f32> {
        let mut e = Engine::new(48000, 2);
        e.set_layout(vbap::LayoutId::Dolby7_1_4).unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/hrtf/apple-hrtfhq-v1/hrtf-dense/hrtf-set.json");
        e.replace_hrtf(hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(), 0.)
            .unwrap();
        e.set_spatial_enhancement(false).unwrap();
        e.set_direct_objects(true).unwrap();
        e.set_directional_hrtf(true);
        e.set_output_volume(1., true);
        e.set_master_preamp_db(0.).unwrap();
        e.disable_fast_objects = !fast;
        e.direct_mix = 1.;
        e.output_active = true;
        e.paused = false;
        let settings = super::Settings {
            bass_db: gain,
            treble_db: gain,
            ..Default::default()
        };
        e.set_listening_profile(settings).unwrap();
        let mut source = Source {
            kind: SourceKind::Object,
            position: [0., 1., 0.],
            gain: 1.,
            target_gain: 1.,
            availability: 1.,
            availability_target: 1.,
            ..Default::default()
        };
        let frames = 96000;
        let input: Vec<f32> = (0..frames)
            .map(|i| {
                let t = std::f32::consts::TAU * i as f32 / 48000.;
                0.002 * ((40. * t).sin() + (8000. * t).sin())
            })
            .collect();
        source.samples.write(0, 0, &input);
        e.sources.insert("audition".into(), source);
        e.route_source_now("audition", 0).unwrap();
        let mut pcm = vec![0.; frames * 2];
        for block in pcm.chunks_mut(173 * 2) {
            e.render_into(block, 2);
        }
        assert_eq!(e.peak_guard.diagnostic_gain(), 1.);
        e.reset_seek(0, 24000);
        assert_eq!(
            e.auxiliary_balance.listening, settings,
            "seek erased calibration"
        );
        e.authored.enabled = true;
        assert!(e.set_listening_profile(Default::default()).is_err());
        assert_eq!(e.auxiliary_balance.listening, settings);
        pcm
    }
    fn tone(pcm: &[f32], hz: f64, ear: usize) -> f64 {
        let (mut re, mut im) = (0., 0.);
        for (i, p) in pcm[144000..].chunks_exact(2).enumerate() {
            let t = std::f64::consts::TAU * hz * i as f64 / 48000.;
            re += f64::from(p[ear]) * t.cos();
            im += f64::from(p[ear]) * t.sin();
        }
        re.hypot(im)
    }
    #[test]
    fn real_mixer_applies_shelves_and_preserves_ear_ratio_in_both_paths() {
        for fast in [false, true] {
            let baseline = render(0., fast);
            for gain in [-3., 3.] {
                let output = render(gain, fast);
                assert!(output.iter().all(|x| x.is_finite()));
                for hz in [40., 8000.] {
                    let db: [f64; 2] = std::array::from_fn(|ear| {
                        20. * (tone(&output, hz, ear) / tone(&baseline, hz, ear)).log10()
                    });
                    assert!((db[0] - db[1]).abs() < 0.03, "ear ratio changed: {db:?}");
                    // A shelf's nominal gain is its asymptote; 8 kHz is still in
                    // the 6 kHz transition. Compare PCM with the actual transfer.
                    let settings = super::Settings {
                        bass_db: gain,
                        treble_db: gain,
                        ..Default::default()
                    };
                    let filters = super::Processor::filters(48000, settings);
                    let expected = filters[0]
                        .iter()
                        .map(|f| {
                            let [re, im] = f.frequency_response(48000, hz);
                            20. * re.hypot(im).log10()
                        })
                        .sum::<f64>();
                    assert!(
                        expected.abs() > 2.,
                        "test tone must exercise a substantial shelf"
                    );
                    assert!(db.iter().all(|v|(*v-expected).abs()<0.06),"shelves missing in actual mixer: fast={fast} hz={hz} expected={expected} actual={db:?}");
                }
            }
        }
    }
}
