//! Bounded fixed-reference convolution for programme metering, never a device sink.
//! Sum source spectra before the two inverse transforms; no PCM history survives
//! beyond the fixed FIR span. Live rendering continues through its existing graph.
use std::sync::Arc;
use rustfft::{Fft,FftPlanner,num_complex::Complex32};
pub struct StereoReferenceConvolver {
    partition:usize,parts:usize,cursor:usize,
    filters:Vec<[Vec<Vec<Complex32>>;2]>,history:Vec<Vec<Vec<Complex32>>>,previous:Vec<Vec<f32>>,
    forward:Arc<dyn Fft<f32>>,inverse:Arc<dyn Fft<f32>>,scratch:Vec<Complex32>,
    left:Vec<Complex32>,right:Vec<Complex32>,pending:[Vec<f32>;2],
}
impl StereoReferenceConvolver {
    pub fn new(filters:&[(Vec<f32>,Vec<f32>)],partition:usize)->Result<Self,String>{
        if partition==0 || !partition.is_power_of_two() || partition>4096 || filters.is_empty() || filters.len()>crate::vbap::MAX_BUS_COUNT{return Err("invalid reference convolution shape".into());}
        if filters.iter().any(|(l,r)|l.is_empty()||l.len()!=r.len()||l.len()>65536||l.iter().chain(r).any(|v|!v.is_finite())){return Err("invalid reference filters".into());}
        let parts=filters.iter().map(|f|f.0.len().div_ceil(partition)).max().unwrap();let n=partition*2;
        if filters.len().saturating_mul(parts).saturating_mul(n).saturating_mul(3)>4_194_304{return Err("reference convolution budget exceeded".into());}
        let mut planner=FftPlanner::new();let forward=planner.plan_fft_forward(n);let inverse=planner.plan_fft_inverse(n);
        let mut scratch=vec![Complex32::ZERO;forward.get_inplace_scratch_len().max(inverse.get_inplace_scratch_len())];
        let mut prepared=Vec::with_capacity(filters.len());
        for(l,r)in filters{
            let mut ears:[Vec<Vec<Complex32>>;2]=std::array::from_fn(|_|Vec::with_capacity(parts));
            for(ear,ir)in [l,r].into_iter().enumerate(){for part in 0..parts{
                let mut spectrum=vec![Complex32::ZERO;n];let at=part*partition;
                if at<ir.len(){for(dst,&x)in spectrum.iter_mut().zip(ir[at..].iter().take(partition)){dst.re=x;}}
                forward.process_with_scratch(&mut spectrum,&mut scratch);ears[ear].push(spectrum);
            }}prepared.push(ears);
        }
        Ok(Self{partition,parts,cursor:0,filters:prepared,history:vec![vec![vec![Complex32::ZERO;n];parts];filters.len()],previous:vec![vec![0.;partition];filters.len()],forward,inverse,scratch,left:vec![Complex32::ZERO;n],right:vec![Complex32::ZERO;n],pending:std::array::from_fn(|_|vec![0.;partition])})
    }
    pub fn process_block(&mut self,input:&[&[f32]],left:&mut[f32],right:&mut[f32])->Result<(),String>{
        let p=self.partition;let n=p*2;
        if input.len()!=self.filters.len()||left.len()!=p||right.len()!=p||input.iter().any(|v|v.len()!=p||v.iter().any(|x|!x.is_finite())){return Err("invalid reference PCM block".into());}
        self.left.fill(Complex32::ZERO);self.right.fill(Complex32::ZERO);
        for(channel,samples)in input.iter().enumerate(){
            let current=&mut self.history[channel][self.cursor];
            for i in 0..p{current[i]=Complex32::new(self.previous[channel][i],0.);current[p+i]=Complex32::new(samples[i],0.);}
            self.previous[channel].copy_from_slice(samples);self.forward.process_with_scratch(current,&mut self.scratch);
            for part in 0..self.parts{
                let row=&self.history[channel][(self.cursor+self.parts-part)%self.parts];let filters=&self.filters[channel];
                for i in 0..n{self.left[i]+=row[i]*filters[0][part][i];self.right[i]+=row[i]*filters[1][part][i];}
            }
        }
        self.inverse.process_with_scratch(&mut self.left,&mut self.scratch);self.inverse.process_with_scratch(&mut self.right,&mut self.scratch);
        let scale=1.0/n as f32;
        for i in 0..p{left[i]=self.pending[0][i];right[i]=self.pending[1][i];self.pending[0][i]=self.left[p+i].re*scale;self.pending[1][i]=self.right[p+i].re*scale;}
        self.cursor=(self.cursor+1)%self.parts;Ok(())
    }
}
/// Prepare the exact static bed routing used by the live graph, including
/// VBAP folds for labels absent from the requested target speaker layout.
pub struct BedReferenceFilters {pub filters:Vec<(Vec<f32>,Vec<f32>)>,pub inputs:Vec<usize>,pub lfe_inputs:Vec<(usize,f32)>}
pub fn prepare_bed_filters(set:&crate::hrtf::NativeHrtfSet,layout:crate::vbap::LayoutId,labels:&[String])->Result<BedReferenceFilters,String>{
 let solver=crate::vbap::VbapSolver::with_layout(layout);let mut buses=Vec::new();
 for i in 0..solver.bus_count(){let(a,e)=solver.speaker_direction(i);let name=crate::vbap::speakers(layout)[i].name;buses.push(set.mixed_speaker(name,layout.as_str(),a as f64,e as f64,0.)?);}
 let mut result=BedReferenceFilters{filters:vec![],inputs:vec![],lfe_inputs:vec![]};
 for(channel,label)in labels.iter().enumerate(){
  if !matches!(label.as_str(),"L"|"R"|"C"|"LFE"|"Ls"|"Rs"|"Lb"|"Rb"|"Tfl"|"Tfr"|"Tbl"|"Tbr"|"Tsl"|"Tsr"){return Err("unverified reference bed label".into());}
  let route=crate::bed_route(label,&solver);if route.lfe!=0.{result.lfe_inputs.push((channel,route.lfe));}
  if route.buses.iter().all(|g|*g==0.){continue;}
  let length=buses.iter().map(|(l,r)|l.len().max(r.len())).max().unwrap_or(1);let mut left=vec![0.;length];let mut right=vec![0.;length];
  for(i,(l,r))in buses.iter().enumerate(){let gain=route.buses[i];if gain==0.{continue;}for(j,&x)in l.iter().enumerate(){left[j]+=gain*x;}for(j,&x)in r.iter().enumerate(){right[j]+=gain*x;}}
  let end=left.iter().zip(&right).rposition(|(l,r)|*l!=0.||*r!=0.).map_or(1,|i|i+1);left.truncate(end);right.truncate(end);
  result.filters.push((left,right));result.inputs.push(channel);
 }
 if result.inputs.is_empty(){return Err("reference has no directional bed channels".into());}Ok(result)
}
/// The same LFE filter/compressor as the live graph, with reference-clock delay.
pub struct ReferenceLfe {lowpass:crate::dsp::Lr4Lowpass,compressor:crate::dsp::MonoCompressor,delay:Vec<f32>,cursor:usize}
impl ReferenceLfe {
 pub fn new(rate:u32,delay:usize)->Result<Self,String>{
  if delay==0||delay>16384{return Err("invalid reference LFE delay".into());}
  Ok(Self{lowpass:crate::dsp::Lr4Lowpass::new(rate,120.)?,compressor:crate::dsp::MonoCompressor::lfe(rate),delay:vec![0.;delay],cursor:0})
 }
 pub fn add_block(&mut self,input:&[f32],left:&mut[f32],right:&mut[f32])->Result<(),String>{
  if input.len()!=left.len()||input.len()!=right.len(){return Err("invalid reference LFE block".into());}
  for i in 0..input.len(){let x=self.delay[self.cursor]*0.5;self.delay[self.cursor]=self.compressor.process(self.lowpass.process(input[i]));self.cursor=(self.cursor+1)%self.delay.len();left[i]+=x;right[i]+=x;}Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn reference_matrix_matches_direct_convolution_across_partition_boundaries(){
  let p=64;let filters:Vec<_>=(0..3).map(|ch|((0..p*2+7).map(|i|((i+ch+1)as f32*0.37).cos()*0.01).collect::<Vec<_>>(),(0..p*2+7).map(|i|((i+ch+2)as f32*0.19).sin()*0.013).collect::<Vec<_>>())).collect();
  let input:Vec<Vec<f32>>=(0..3).map(|ch|(0..p*8).map(|i|(i as f32*(0.07+ch as f32*0.03)).sin()*0.1).collect()).collect();
  let mut c=StereoReferenceConvolver::new(&filters,p).unwrap();let mut l=vec![0.;p];let mut r=vec![0.;p];
  for block in 0..8{let slices:Vec<_>=input.iter().map(|ch|&ch[block*p..(block+1)*p]).collect();c.process_block(&slices,&mut l,&mut r).unwrap();
   for i in 0..p{let t=block*p+i;for(ear,actual)in [l[i],r[i]].into_iter().enumerate(){let mut expected=0.;if t>=p{let at=t-p;for ch in 0..3{let h=if ear==0{&filters[ch].0}else{&filters[ch].1};for tap in 0..h.len().min(at+1){expected+=input[ch][at-tap]*h[tap];}}}assert!((actual-expected).abs()<2e-6,"block={block} sample={i} ear={ear}: {actual} vs {expected}");}}
  }
 }
 #[test]fn invalid_reference_allocations_and_pcm_are_rejected(){
  assert!(StereoReferenceConvolver::new(&[],64).is_err());assert!(StereoReferenceConvolver::new(&[(vec![f32::NAN],vec![0.])],64).is_err());
  let mut c=StereoReferenceConvolver::new(&[(vec![1.],vec![1.])],64).unwrap();assert!(c.process_block(&[&[0.;63]],&mut[0.;64],&mut[0.;64]).is_err());
 }
}
