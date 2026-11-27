//! Causal scene-domain masking evidence. Fixed state, no audio buffering.
//! The detector sees unprocessed world-coordinate references; head motion and
//! already applied enhancement cannot feed back into its measurements.
use crate::dsp::Biquad;
pub(crate) struct Detector {
    filters: [[[Biquad; 2]; 4]; 2],
    clean_filters: [Biquad;2],
    power: [[f64; 2]; 2],
    alpha: f64,
    samples: u64,
    rate: u32,
}
impl Detector {
    pub fn new(rate: u32) -> Self {
        let pair = [
            Biquad::butterworth_highpass(rate, 1800f32.min(rate as f32 * 0.15)).unwrap(),
            Biquad::butterworth_highpass(rate, 5000f32.min(rate as f32 * 0.3)).unwrap(),
        ];
        Self {
            filters: [[pair; 4]; 2],
            clean_filters: pair,
            power: [[0.; 2]; 2],
            alpha: 1. - (-1. / (f64::from(rate.max(1)) * 0.12)).exp(),
            samples: 0,
            rate,
        }
    }
    pub fn observe(&mut self, groups: [[f64; 4]; 2]) {
        for (g, group) in groups.iter().enumerate() {
            let mut power = [0.; 2];
            for (a, x) in group.iter().enumerate() {
                let narrowed = *x as f32;
                let x = if narrowed.is_finite() { narrowed } else { 0. };
                let h = self.filters[g][a][0].process(x);
                let air = self.filters[g][a][1].process(h);
                let detail = h - air;
                if !h.is_finite() || !air.is_finite() || !detail.is_finite() {
                    self.filters[g][a] = self.clean_filters;
                    self.power[g] = [0.0; 2];
                    continue;
                }
                for (i, y) in [detail, air].iter().enumerate() {
                    power[i] += f64::from(*y).powi(2);
                }
            }
            for (i, p) in power.iter().enumerate() {
                self.power[g][i] += self.alpha * (*p - self.power[g][i]);
            }
        }
        self.samples = self.samples.saturating_add(1);
    }
    pub fn targets(&self, programme: f64) -> [f32; 2] {
        if self.samples < u64::from(self.rate) / 10 || !programme.is_finite() || programme <= 0. {
            return [0.; 2];
        }
        std::array::from_fn(|i| {
            let [f, a] = [self.power[0][i], self.power[1][i]];
            if a * programme.powi(2) < 10f64.powf(-78. / 10.) || f <= 1e-24 {
                return 0.;
            }
            // Require same-band foreground dominance. No broadband energy
            // proxy: a bass-heavy foreground must not brighten clear cymbals.
            let deficit = (10. * (f / a.max(1e-30)).log10() - 3.).max(0.);
            (deficit / (deficit + 6.)) as f32
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn masking_recovers_after_out_of_range_finite_reference() {
        for corrupt in [f64::MAX, f64::from(f32::MAX)] {
            let mut d = Detector::new(48000);
            d.observe([[corrupt; 4]; 2]);
            for i in 0..96000 {
                let x = (std::f64::consts::TAU * 7000.0 * i as f64 / 48000.0).sin();
                d.observe([[0.2 * x, 0.0, 0.0, 0.0], [0.02 * x, 0.0, 0.0, 0.0]]);
            }
            assert!(
                d.targets(1.0)[1] > 0.6,
                "one finite outlier permanently poisoned the masking filters"
            );
        }
    }
    fn run(front: f32, aux: f32) -> [f32; 2] {
        let mut d = Detector::new(48000);
        for i in 0..96000 {
            let s = |hz: f32| (std::f32::consts::TAU * hz * i as f32 / 48000.).sin();
            d.observe([
                [f64::from(0.2 * s(front)), 0., 0., 0.],
                [f64::from(0.02 * s(aux)), 0., 0., 0.],
            ]);
        }
        d.targets(1.)
    }
    #[test]
    fn bass_does_not_invent_high_band_masking() {
        let unrelated = run(80., 7000.);
        assert!(unrelated[1] < 0.01, "{unrelated:?}");
        let masked = run(7000., 7000.);
        assert!(masked[1] > 0.6, "{masked:?}");
    }
    #[test]
    fn silence_and_invalid_references_never_request_enhancement() {
        let mut d = Detector::new(48000);
        for _ in 0..24000 {
            d.observe([[f64::NAN; 4]; 2]);
        }
        assert_eq!(d.targets(1.), [0.; 2]);
        assert_eq!(d.targets(f64::NAN), [0.; 2]);
    }
}
