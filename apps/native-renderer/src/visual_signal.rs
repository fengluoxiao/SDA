//! Passive visual meters from PCM consumed by the audio callback. No PCM is
//! retained, allocated, modified or sent to the UI; only six scalar levels.
pub(crate) struct VisualSignalMeter {
    lowpass: [[f32; 4]; 2],
    sums: [f64; 6],
}
impl Default for VisualSignalMeter {
    fn default() -> Self { Self { lowpass: [[0.0; 4]; 2], sums: [0.0; 6] } }
}
impl VisualSignalMeter {
    pub fn reset(&mut self) { *self = Self::default(); }
    pub fn begin(&mut self) { self.sums = [0.0; 6]; }
    pub fn push(&mut self, frame: [f32; 2]) {
        // One-pole lowpasses at 200/500/2000/6000Hz, at the renderer's 48kHz.
        // Independent L/R states preserve energy in out-of-phase stereo.
        const A: [f32; 4] = [0.0258404, 0.0633540, 0.2303346, 0.5440619];
        for (channel, sample) in frame.into_iter().enumerate() {
            if !sample.is_finite() { continue; }
            // Bound only the passive measurement; the caller writes the original PCM.
            let sample = sample.clamp(-4.0,4.0);
            self.sums[0] += (sample as f64).powi(2);
            let state = &mut self.lowpass[channel];
            for i in 0..4 { state[i] += A[i] * (sample - state[i]); }
            let bands = [state[0], state[1]-state[0], state[2]-state[1], state[3]-state[2], sample-state[3]];
            for (i, value) in bands.into_iter().enumerate() { self.sums[i+1] += (value as f64).powi(2); }
        }
    }
    pub fn finish(&mut self, requested: usize, consumed: usize) -> [f32; 6] {
        let denominator = (requested.max(1) * 2) as f64;
        let result = if consumed == 0 { [0.0; 6] } else { self.sums.map(|sum| {
            let rms = (sum / denominator).sqrt();
            if rms <= 1e-8 { 0.0 } else { (((20.0*rms.log10()+70.0)/70.0).clamp(0.0,1.0)*255.0) as f32 }
        }) };
        if consumed < requested { self.lowpass = [[0.0;4];2]; }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn measure(hz:f32,antiphase:bool)->[f32;6] {
        let mut meter=VisualSignalMeter::default();let mut levels=[0.0;6];
        for block in 0..30 { meter.begin();for i in 0..512 {let sample=(std::f32::consts::TAU*hz*(block*512+i) as f32/48000.0).sin()*0.2;meter.push([sample,if antiphase{-sample}else{sample}]);}levels=meter.finish(512,512); }
        levels
    }
    #[test] fn silence_and_underrun_are_zero(){let mut m=VisualSignalMeter::default();m.begin();assert_eq!(m.finish(512,0),[0.0;6]);for _ in 0..512{m.push([0.0;2]);}assert_eq!(m.finish(512,512),[0.0;6]);}
    #[test] fn frequency_buckets_respond_to_actual_pcm(){let bass=measure(80.0,false);let high=measure(10000.0,false);assert!(bass[1]>bass[5]+20.0,"{bass:?}");assert!(high[5]>high[1]+20.0,"{high:?}");assert!(bass[0]>100.0);}
    #[test] fn opposite_phase_stereo_does_not_cancel(){let a=measure(1000.0,false);let b=measure(1000.0,true);for i in 0..6{assert!((a[i]-b[i]).abs()<0.001);}}
    #[test] fn reset_drops_old_track_energy(){let mut m=VisualSignalMeter::default();m.begin();for _ in 0..512{m.push([0.4,-0.4]);}assert!(m.finish(512,512)[0]>0.0);m.reset();m.begin();for _ in 0..512{m.push([0.0;2]);}assert_eq!(m.finish(512,512),[0.0;6]);}
    #[test] fn nonfinite_input_does_not_poison_visual_state(){let mut m=VisualSignalMeter::default();m.begin();m.push([f32::NAN,f32::INFINITY]);assert!(m.finish(1,1).iter().all(|x|x.is_finite()));}
    #[test] fn extreme_finite_input_cannot_poison_following_levels(){let mut m=VisualSignalMeter::default();m.begin();m.push([f32::MAX,-f32::MAX]);assert!(m.finish(1,1).iter().all(|x|x.is_finite()));m.begin();for _ in 0..512{m.push([0.0;2]);}assert!(m.finish(512,512).iter().all(|x|x.is_finite()));}

}
