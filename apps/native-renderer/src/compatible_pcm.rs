//! Codec-provided plain output, clocked alongside the continuously warm scene.
use crate::{MAX_PENDING_SAMPLES, pcm_ring::AbsolutePcmRing};
pub(super) struct CompatiblePcm { ears: [AbsolutePcmRing; 2], coverage:crate::pcm_coverage::PcmCoverage }
impl CompatiblePcm {
    pub fn new() -> Self { Self { ears: std::array::from_fn(|_| AbsolutePcmRing::new(MAX_PENDING_SAMPLES)), coverage:Default::default() } }
    pub fn can_write(&self, now:u64, start:u64, samples:usize) -> bool {
        self.ears.iter().all(|ring| ring.can_write(now,start,samples))
    }
    pub fn available(&self, at:u64, samples:usize) -> bool {self.coverage.available(at,samples)==samples}
    pub fn write(&mut self, now:u64, start:u64, pcm:&[Vec<f32>;2]) {
        self.coverage.discard_before(now);
        self.coverage.insert(start.max(now),start.saturating_add(pcm[0].len() as u64));
        for (ring,ear) in self.ears.iter_mut().zip(pcm) { ring.write(now,start,ear); }
    }
    pub fn take(&mut self, at:u64) -> Option<[f32;2]> {
        let left=self.ears[0].take(at);let right=self.ears[1].take(at);
        left.zip(right).map(|(l,r)|[l,r])
    }
    pub fn clear(&mut self) { for ring in &mut self.ears {ring.clear();} self.coverage=Default::default(); }
}
