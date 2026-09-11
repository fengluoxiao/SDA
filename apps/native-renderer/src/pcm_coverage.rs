//! Committed PCM time spans, independent of object activity or sample amplitude.
//! A silent decoded frame is available data; an absent frame is not.
use std::collections::BTreeMap;
#[derive(Default)]
pub(super) struct PcmCoverage { ranges:BTreeMap<u64,u64> }
impl PcmCoverage {
    pub fn insert(&mut self, mut start:u64, mut end:u64) {
        if end<=start{return;}
        if let Some((&a,&b))=self.ranges.range(..=start).next_back(){
            if b>=start{start=a;end=end.max(b);self.ranges.remove(&a);}
        }
        loop {
            let next=self.ranges.range(start..=end).next().map(|(&a,&b)|(a,b));
            let Some((a,b))=next else{break};
            end=end.max(b);self.ranges.remove(&a);
        }
        self.ranges.insert(start,end);
    }
    pub fn available(&self, at:u64, limit:usize)->usize {
        self.ranges.range(..=at).next_back().map_or(0,|(_,end)|end.saturating_sub(at).min(limit as u64) as usize)
    }
    pub fn discard_before(&mut self,at:u64){
        while let Some((&start,&end))=self.ranges.first_key_value(){
            if start>=at{break;}
            self.ranges.remove(&start);
            if end>at{self.ranges.insert(at,end);break;}
        }
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn missing_batches_never_advance_but_late_batches_resume(){
        let mut c=PcmCoverage::default();c.insert(0,4096);
        assert_eq!(c.available(4096,1024),0);
        c.insert(8192,12288);assert_eq!(c.available(4096,1024),0);
        c.insert(4096,8192);assert_eq!(c.available(4096,1024),1024);
        c.discard_before(10000);assert_eq!(c.available(9999,1024),0);
        assert_eq!(c.available(12000,1024),288);
        c.discard_before(12288);assert_eq!(c.available(12288,1024),0);
        assert!(c.ranges.is_empty());
    }
    #[test] fn overlapping_and_replayed_ranges_merge(){
        let mut c=PcmCoverage::default();for (a,b) in [(40,60),(0,20),(10,50),(0,60)]{c.insert(a,b);}
        assert_eq!(c.ranges.len(),1);assert_eq!(c.available(0,100),60);
    }
}
