//! ALAC PCM adapter. Port of packages/player/src/alac-stereo-upmix.ts.
//! Input is decoded, resampled 48 kHz stereo; never an Atmos/object reconstruction.
use sda_core::FrameData;
#[derive(Default)]
pub(crate) struct AlacPcm { lfe: f32, live_mix: Option<f32> }
impl AlacPcm {
    /// Fixed-capacity bed for live switching. Dry L/R are bit-identical after
    /// the ramp; unused channels are zero. Never resets the source clock/filter.
    pub fn live_frame(&mut self, pcm: &[f32], upmix: bool, sample_pos: u64) -> Result<FrameData, String> {
        let mut frame = self.frame(pcm, true, sample_pos)?;
        let target = if upmix { 1.0 } else { 0.0 };
        let mut mix = self.live_mix.unwrap_or(target);
        for (i, pair) in pcm.chunks_exact(2).enumerate() {
            mix += (target - mix).clamp(-1.0/2400.0, 1.0/2400.0);
            for ch in 0..12 {
                let dry = if ch < 2 { pair[ch] } else { 0.0 };
                frame.channels[ch][i] = if mix == 0.0 { dry } else if mix == 1.0 {
                    frame.channels[ch][i]
                } else { dry + (frame.channels[ch][i] - dry) * mix };
            }
        }
        self.live_mix = Some(mix);
        Ok(frame)
    }
    pub fn live_channels(&self, upmix: bool) -> usize {
        if upmix || self.live_mix.unwrap_or(0.0) > 0.0 { 12 } else { 2 }
    }
    pub fn frame(&mut self, pcm: &[f32], upmix: bool, sample_pos: u64) -> Result<FrameData, String> {
        if pcm.is_empty() || pcm.len() % 2 != 0 || pcm.len() > 65536 || pcm.iter().any(|v| !v.is_finite()) {
            return Err("invalid ALAC stereo PCM".into());
        }
        let labels: &[&str] = if upmix { &["L","R","C","LFE","Ls","Rs","Lb","Rb","Tfl","Tfr","Trl","Trr"] } else { &["L","R"] };
        let mut channels = vec![Vec::with_capacity(pcm.len()/2); labels.len()];
        let alpha = 1.0 - (-2.0 * std::f32::consts::PI * 120.0 / 48000.0).exp();
        for pair in pcm.chunks_exact(2) {
            let (l,r) = (pair[0],pair[1]);
            if !upmix { channels[0].push(l); channels[1].push(r); continue; }
            let mono = (l+r)*0.5; let side = (l-r)*0.5;
            self.lfe += alpha * (mono-self.lfe);
            let values = [l*0.8,r*0.8,mono*0.5,self.lfe*0.25,side*0.34,-side*0.34,side*0.2,-side*0.2,side*0.14,-side*0.14,side*0.1,-side*0.1];
            for (channel,value) in channels.iter_mut().zip(values) { channel.push(value); }
        }
        Ok(FrameData { codec:"alac", sample_rate:48000, sample_pos, channels,
            labels:labels.iter().map(|v| (*v).into()).collect(), raw_bed_labels:vec!["L".into(),"R".into()],
            events:vec![], object_channels:vec![], program_loudness:None, ramp_duration:0 })
    }
}
pub(crate) fn is_upmixed(f: &FrameData) -> bool {
    f.codec == "alac" && f.channels.len() == 12 && f.raw_bed_labels == ["L", "R"]
        && f.events.is_empty() && f.object_channels.is_empty()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_switch_is_chunk_invariant_reversible_and_keeps_every_sample() {
        fn run(chunk: usize) -> Vec<Vec<f32>> {
            let mut a = AlacPcm::default();
            let mut output = vec![Vec::new();12];
            let mut at = 0;
            for wet in [false,true,false,true,false] {
                let pcm: Vec<f32> = (0..5000).flat_map(|i| [0.3+(i as f32)*0.00001,-0.2]).collect();
                for block in pcm.chunks(chunk*2) {
                    let frame = a.live_frame(block,wet,at).unwrap();
                    assert_eq!(frame.sample_pos,at);
                    at += (block.len()/2) as u64;
                    for (dst,src) in output.iter_mut().zip(frame.channels) { dst.extend(src); }
                }
                if !wet {
                    for i in 3000..5000 {
                        assert_eq!(output[0][at as usize-5000+i],pcm[i*2]);
                        assert_eq!(output[1][at as usize-5000+i],pcm[i*2+1]);
                        assert!(output[2..].iter().all(|c| c[at as usize-5000+i] == 0.0));
                    }
                    assert_eq!(a.live_channels(false),2);
                }
            }
            assert!(output.iter().all(|c| c.len()==25000));
            // Constant-ish source: the crossfade must not create a gain step.
            assert!(output[0][5000..7500].windows(2).all(|w| (w[1]-w[0]).abs()<0.001));
            output
        }
        assert_eq!(run(5000),run(127));
    }
    #[test] fn stereo_is_unchanged_and_mono_has_no_fake_height() {
        let pcm = [0.4,0.4,0.2,0.2];
        let dry = AlacPcm::default().frame(&pcm,false,0).unwrap();
        assert_eq!(dry.channels,vec![vec![0.4,0.2],vec![0.4,0.2]]);
        let wet = AlacPcm::default().frame(&pcm,true,0).unwrap();
        assert!(is_upmixed(&wet));
        assert!(wet.channels[4..].iter().flatten().all(|v| *v == 0.0));
        assert!(wet.events.is_empty());
    }
    #[test] fn upmix_is_chunk_invariant_and_has_expected_channel_order() {
        let pcm: Vec<f32> = (0..4096).map(|i| (i as f32*0.03).sin()*0.4).collect();
        let full = AlacPcm::default().frame(&pcm,true,0).unwrap();
        let mut adapter = AlacPcm::default();
        let a = adapter.frame(&pcm[..2048],true,0).unwrap();
        let b = adapter.frame(&pcm[2048..],true,1024).unwrap();
        for i in 0..12 { let mut joined=a.channels[i].clone(); joined.extend_from_slice(&b.channels[i]); assert_eq!(joined,full.channels[i]); }
        assert_eq!(full.labels[4],"Ls"); assert_eq!(full.labels[6],"Lb");
        let side=(pcm[0]-pcm[1])*0.5;
        assert_eq!(full.channels[4][0],side*0.34); assert_eq!(full.channels[6][0],side*0.2);
    }
    #[test] fn rejects_malformed_pcm() {
        for pcm in [vec![],vec![0.0],vec![f32::NAN,0.0],vec![f32::INFINITY,0.0],vec![0.0;65538]] {
            assert!(AlacPcm::default().frame(&pcm,true,0).is_err());
        }
    }
}
