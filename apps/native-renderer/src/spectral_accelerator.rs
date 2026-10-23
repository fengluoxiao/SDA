//! Bounded, optional FP32 HRTF spectral-MAC offload. CPU FFTs, filter motion,
//! source state and the output guard remain in the renderer. Android selects an
//! explicit NNAPI accelerator. iOS requests Core ML CPU/NE and labels its
//! framework-managed, unverified per-prediction hardware placement separately.
use rustfft::num_complex::Complex32;

#[cfg(any(target_os="android",test))]
pub mod cooperation;
mod model_graph;
pub(crate) use model_graph::{Graph, Shape};
/// A mapped packet owns a strong lifetime token. Its unique mutable slice is
/// moved to the compute worker while the GPU uses it; the worker retires the
/// underlying context only after all packet tokens have gone away.
pub(crate) struct FloatStorage {inner:FloatBacking}
enum FloatBacking {Owned(Vec<f32>),Mapped{pointer:std::ptr::NonNull<f32>,len:usize,_owner:std::sync::Arc<dyn Send+Sync>}}
unsafe impl Send for FloatStorage {}
impl FloatStorage {
 fn owned(len:usize)->Self{Self{inner:FloatBacking::Owned(vec![0.;len])}}
 unsafe fn mapped(pointer:std::ptr::NonNull<f32>,len:usize,owner:std::sync::Arc<dyn Send+Sync>)->Self{Self{inner:FloatBacking::Mapped{pointer,len,_owner:owner}}}
 #[cfg(test)]fn clone(&self)->Vec<f32>{self.to_vec()}
}
impl std::ops::Deref for FloatStorage {type Target=[f32];fn deref(&self)->&[f32]{match &self.inner {FloatBacking::Owned(v)=>v,FloatBacking::Mapped{pointer,len,..}=>unsafe{std::slice::from_raw_parts(pointer.as_ptr(),*len)}}}}
impl std::ops::DerefMut for FloatStorage {fn deref_mut(&mut self)->&mut[f32]{match &mut self.inner {FloatBacking::Owned(v)=>v,FloatBacking::Mapped{pointer,len,..}=>unsafe{std::slice::from_raw_parts_mut(pointer.as_ptr(),*len)}}}}
#[cfg(test)]impl std::fmt::Debug for FloatStorage{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{std::fmt::Debug::fmt(&self.deref_for_debug(),f)}}
#[cfg(test)]impl FloatStorage{fn deref_for_debug(&self)->&[f32]{self}}
#[cfg(test)]impl PartialEq for FloatStorage{fn eq(&self,other:&Self)->bool{**self==**other}}
pub(crate) struct Batch {
    pub shape: Shape,
    pub input: FloatStorage,
    pub output: FloatStorage,
    pub reference: Vec<f32>,
    pub resident: bool,
    pub filter_dirty: Vec<u8>,
    pub packed_parts:Vec<u8>,
    previous_parts:Vec<u8>,
    reference_rows:Vec<u8>,
    filter_keys: Vec<Option<(u64,u64,bool)>>,
    pub cpu_work_ns: u64,
    pub worker_cpu_ns: u64,
    pub checked_parity: Option<bool>,
    pub gpu_finite_verified:bool,
}
impl Batch {
    pub fn new(shape: Shape) -> Self {
        Self {
            shape,
            input: FloatStorage::owned(shape.plane() * 6 * shape.parts),
            output: FloatStorage::owned(shape.plane() * 4),
            reference: vec![0.; shape.plane() * 4],
            resident:false, filter_dirty:vec![0;shape.rows],filter_keys:vec![None;shape.rows],packed_parts:vec![0;shape.rows],previous_parts:vec![0;shape.rows],reference_rows:vec![0;shape.rows],
            cpu_work_ns:0,worker_cpu_ns:0,checked_parity:None,gpu_finite_verified:false,
        }
    }
    pub unsafe fn mapped(shape:Shape,input:std::ptr::NonNull<f32>,output:std::ptr::NonNull<f32>,owner:std::sync::Arc<dyn Send+Sync>)->Self {
        Self{shape,input:unsafe{FloatStorage::mapped(input,shape.plane()*6*shape.parts,owner.clone())},output:unsafe{FloatStorage::mapped(output,shape.plane()*4,owner)},reference:vec![0.;shape.plane()*4],resident:true,filter_dirty:vec![0;shape.rows],filter_keys:vec![None;shape.rows],packed_parts:vec![0;shape.rows],previous_parts:vec![0;shape.rows],reference_rows:vec![0;shape.rows],cpu_work_ns:0,worker_cpu_ns:0,checked_parity:None,gpu_finite_verified:false}
    }
    pub fn resident(shape:Shape)->Self {let mut batch=Self::new(shape);batch.resident=true;batch}
    pub fn row(&mut self,row:usize,id:u64,revision:u64,target:bool){
        if !self.resident{return;}let key=Some((id,revision,target));
        if self.filter_keys[row]!=key {self.filter_dirty[row]=1;self.filter_keys[row]=key;}
    }
    pub fn clear(&mut self) {
        if self.resident {self.filter_dirty.fill(0);self.packed_parts.fill(0);}else{self.input.fill(0.);self.output.fill(0.);self.reference.fill(0.);}
        self.cpu_work_ns=0;self.worker_cpu_ns=0;self.checked_parity=None;self.gpu_finite_verified=false;
    }
    pub fn finish_packet(&mut self){
        if !self.resident{return;}let s=self.shape;
        for row in 0..s.rows {
            let missing=self.previous_parts[row]&!self.packed_parts[row];
            for part in 0..s.parts {if missing&(1<<part)!=0{let x=(part*s.rows+row)*s.bins*2;self.input[x..x+s.bins*2].fill(0.);}}
            if self.packed_parts[row]==0&&self.reference_rows[row]!=0 {
                let base=row*s.bins*4;self.reference[base..base+s.bins*4].fill(0.);
                self.reference_rows[row]=0;
            }
        }
        self.previous_parts.copy_from_slice(&self.packed_parts);
    }
    pub fn pack(
        &mut self,
        row: usize,
        part: usize,
        input: &[Complex32],
        left: &[Complex32],
        right: &[Complex32],
    ) {
        let s = self.shape;
        assert!(row < s.rows && part < s.parts);
        assert!(input.len() >= s.bins && left.len() >= s.bins && right.len() >= s.bins);
        if self.resident {
            self.packed_parts[row]|=1<<part;
            let x=(part*s.rows+row)*s.bins*2;
            assert!(s.bins<=self.input.len()/2 && x<=self.input.len()-s.bins*2);
            // Complex32 is repr(C), two adjacent f32s, as in the existing SIMD path.
            unsafe { std::ptr::copy_nonoverlapping(input.as_ptr().cast::<f32>(),self.input.as_mut_ptr().add(x),s.bins*2); }
            if self.filter_dirty[row]!=0 {
                let base=2*s.plane()*s.parts+(row*s.parts+part)*s.bins*4;
                for bin in 0..s.bins {let b=base+bin*4;self.input[b]=left[bin].re;self.input[b+1]=left[bin].im;self.input[b+2]=right[bin].re;self.input[b+3]=right[bin].im;}
            }
            return;
        }
        let base = part * 6 * s.plane() + row * s.bins;
        for (field, values, imaginary) in [
            (0, input, false),
            (1, input, true),
            (2, left, false),
            (3, left, true),
            (4, right, false),
            (5, right, true),
        ] {
            let dst = &mut self.input[base + field * s.plane()..][..s.bins];
            for (dst, value) in dst.iter_mut().zip(values) {
                *dst = if imaginary { value.im } else { value.re };
            }
        }
    }
    pub fn store_reference(&mut self, row: usize, left: &[Complex32], right: &[Complex32]) {
        let s = self.shape;
        if self.resident{self.reference_rows[row]=1;}
        assert!(row < s.rows);
        if self.resident {
            let base=row*s.bins*4;
            assert!(left.len()>=s.bins && right.len()>=s.bins && s.bins<=self.reference.len()/4 && base<=self.reference.len()-s.bins*4);
            unsafe {std::ptr::copy_nonoverlapping(left.as_ptr().cast::<f32>(),self.reference.as_mut_ptr().add(base),s.bins*2);std::ptr::copy_nonoverlapping(right.as_ptr().cast::<f32>(),self.reference.as_mut_ptr().add(base+s.bins*2),s.bins*2);}
            return;
        }
        for (ear, values) in [left, right].into_iter().enumerate() {
            for (bin, value) in values.iter().take(s.bins).enumerate() {
                self.reference[(ear * 2) * s.plane() + row * s.bins + bin] = value.re;
                self.reference[(ear * 2 + 1) * s.plane() + row * s.bins + bin] = value.im;
            }
        }
    }
    pub fn load_output(&self, row: usize, left: &mut [Complex32], right: &mut [Complex32]) {
        let s = self.shape;
        assert!(row < s.rows);
        // inverse_stereo writes every negative left bin from the positive half
        // and derives the complete right output; clearing the full FFTs is unnecessary.
        if self.resident {
            let base=row*s.bins*4;
            assert!(left.len()>=s.bins && right.len()>=s.bins && s.bins<=self.output.len()/4 && base<=self.output.len()-s.bins*4);
            unsafe {std::ptr::copy_nonoverlapping(self.output.as_ptr().add(base),left.as_mut_ptr().cast::<f32>(),s.bins*2);std::ptr::copy_nonoverlapping(self.output.as_ptr().add(base+s.bins*2),right.as_mut_ptr().cast::<f32>(),s.bins*2);}
            return;
        }
        for (ear, values) in [left, right].into_iter().enumerate() {
            for (bin, value) in values.iter_mut().take(s.bins).enumerate() {
                *value = Complex32::new(
                    self.output[(ear * 2) * s.plane() + row * s.bins + bin],
                    self.output[(ear * 2 + 1) * s.plane() + row * s.bins + bin],
                );
            }
        }
    }
    pub fn reference_matches(&self) -> bool {
        let mut energy = 0f64;
        let mut error = 0f64;
        let mut peak = 0f32;
        let mut delta = 0f32;
        for (&actual, &expected) in self.output.iter().zip(&self.reference) {
            if !actual.is_finite() || !expected.is_finite() {
                return false;
            }
            let d = actual as f64 - expected as f64;
            energy += (expected as f64).powi(2);
            error += d * d;
            peak = peak.max(expected.abs());
            delta = delta.max(d.abs() as f32);
        }
        // <= -120 dB RMS error relative to reference; also bound individual bins.
        // No absolute floor: a quiet signal must not qualify after being rounded away.
        error <= energy * 1e-12 && delta <= peak * 4e-6
    }
    #[cfg(test)]
    pub fn scalar_reference(&mut self) {
        if self.resident {
            let s=self.shape;let n=s.plane();self.output.fill(0.);
            for row in 0..s.rows {for bin in 0..s.bins {let i=row*s.bins+bin;for part in 0..s.parts {
                let x=(part*s.rows+row)*s.bins*2+bin*2;let h=2*n*s.parts+((row*s.parts+part)*s.bins+bin)*4;
                let xr=self.input[x];let xi=self.input[x+1];
                let out=row*s.bins*4+bin*2;
                self.output[out]+=xr*self.input[h]-xi*self.input[h+1];self.output[out+1]+=xr*self.input[h+1]+xi*self.input[h];
                self.output[out+s.bins*2]+=xr*self.input[h+2]-xi*self.input[h+3];self.output[out+s.bins*2+1]+=xr*self.input[h+3]+xi*self.input[h+2];
            }}}
            return;
        }
        let n = self.shape.plane();
        self.output.fill(0.);
        for part in 0..self.shape.parts {
            let v = &self.input[part * 6 * n..];
            for i in 0..n {
                for ear in 0..2 {
                    let hr = v[(2 + 2 * ear) * n + i];
                    let hi = v[(3 + 2 * ear) * n + i];
                    self.output[(2 * ear) * n + i] += v[i] * hr - v[n + i] * hi;
                    self.output[(2 * ear + 1) * n + i] += v[i] * hi + v[n + i] * hr;
                }
            }
        }
    }
}

#[cfg(any(target_os = "android", all(target_os = "ios", feature = "coreml-audio"), test))]
mod coordinator;
#[cfg(any(all(target_os = "ios", feature = "coreml-audio"), test))]
mod coreml_model;
#[cfg(all(target_os = "ios", feature = "coreml-audio"))]
#[path = "coreml_spectral.rs"]
mod coreml;
#[cfg(all(target_os = "ios", feature = "coreml-audio"))]
#[path = "apple_spectral.rs"]
mod apple;
#[cfg(all(target_os = "ios", feature = "coreml-audio"))]
pub fn set_apple_foreground(active: bool) { apple::transition(active); }
#[cfg(all(target_os = "ios", feature = "coreml-audio"))]
pub fn reset_apple_session() { apple::new_session(); coordinator::release_inactive_backend(); }
#[cfg(target_os = "android")]
#[path = "nnapi_spectral.rs"]
mod nnapi;
#[cfg(target_os = "android")]
#[path = "vulkan_spectral.rs"]
mod vulkan;
#[cfg(any(target_os = "android", test))]
mod litert_model;
#[cfg(any(target_os = "android", test))]
mod hexagon_graph;
#[cfg(target_os = "android")]
#[path = "litert_spectral.rs"]
mod litert;
#[cfg(target_os = "android")]
#[path = "hexagon_spectral.rs"]
mod hexagon;
#[cfg(target_os = "android")]
#[path = "android_spectral.rs"]
mod android;
/// Called once after the Android host prepares optional public runtime assets.
#[cfg(target_os = "android")]
pub fn configure_android_runtime(directory: &str, sdk: u32, preparation: &str) -> Result<(), String> {
    android::configure(directory, sdk, preparation)?;
    start();
    Ok(())
}

#[cfg(any(target_os = "android", all(target_os = "ios", feature = "coreml-audio")))]
pub(crate) use coordinator::{begin, observe_native_block};

#[cfg(all(target_os = "android", feature = "pcm-diagnostic"))]
mod diagnostic;
/// Diagnostic executable only; invokes the real backend without starting audio output.
#[cfg(all(target_os = "android", feature = "pcm-diagnostic"))]
pub fn probe_legacy_runtime(directory: &str) -> Result<serde_json::Value, String> {
    diagnostic::probe_legacy(directory)
}

pub fn start() {
    #[cfg(any(target_os = "android", all(target_os = "ios", feature = "coreml-audio")))]
    coordinator::start();
}
pub fn status() -> serde_json::Value {
    #[cfg(any(target_os = "android", all(target_os = "ios", feature = "coreml-audio")))]
    {
        coordinator::status()
    }
    #[cfg(not(any(target_os = "android", all(target_os = "ios", feature = "coreml-audio"))))]
    {
        serde_json::json!({"state":"unsupported-platform", "npuAudioEnabled":false})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resident_packets_match_original_mac_and_retain_only_versioned_coefficients() {
        for parts in 1..=4 {
            let shape=Shape::bounded(8,parts,17).unwrap();
            let mut old=Batch::new(shape);let mut resident=Batch::resident(shape);
            for frame in 0..3 {
                old.clear();resident.clear();
                for row in 0..shape.rows {
                    let revision=if frame==2&&row==0{2}else{1};
                    resident.row(row,row as u64+1,revision,false);
                    assert_eq!(resident.filter_dirty[row]!=0,frame==0 || (frame==2&&row==0));
                    for part in 0..parts {
                        let x:Vec<_>=(0..shape.bins).map(|i|Complex32::new((i+frame+1)as f32*0.01,-((row+part+1)as f32)*0.03)).collect();
                        let l=vec![Complex32::new(0.2*revision as f32,0.1);shape.bins];let r=vec![Complex32::new(-0.1,0.3);shape.bins];
                        old.pack(row,part,&x,&l,&r);resident.pack(row,part,&x,&l,&r);
                    }
                }
                old.scalar_reference();resident.finish_packet();resident.scalar_reference();
                for row in 0..shape.rows{let mut a=vec![Complex32::new(0.,0.);shape.bins];let mut b=a.clone();let mut c=a.clone();let mut d=a.clone();old.load_output(row,&mut a,&mut b);resident.load_output(row,&mut c,&mut d);assert_eq!(a,c);assert_eq!(b,d);}
            }
            let spectrum=vec![Complex32::new(0.4,-0.3);shape.bins];resident.store_reference(0,&spectrum,&spectrum);
            resident.clear();resident.finish_packet();resident.scalar_reference();assert!(resident.output.iter().all(|v|*v==0.));assert!(resident.reference.iter().all(|v|*v==0.));
            resident.row(0,9999,1,false);assert_eq!(resident.filter_dirty[0],1);
        }
    }
    #[test]
    fn mapped_packet_keeps_backing_alive_until_both_views_are_dropped(){
        struct Owner{input:std::cell::UnsafeCell<Box<[f32]>>,output:std::cell::UnsafeCell<Box<[f32]>>,drops:std::sync::Arc<std::sync::atomic::AtomicUsize>}
        unsafe impl Send for Owner{} unsafe impl Sync for Owner{}
        impl Drop for Owner{fn drop(&mut self){self.drops.fetch_add(1,std::sync::atomic::Ordering::Relaxed);}}
        let shape=Shape::bounded(8,1,9).unwrap();let drops=std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let owner=std::sync::Arc::new(Owner{input:std::cell::UnsafeCell::new(vec![0.;shape.plane()*6].into_boxed_slice()),output:std::cell::UnsafeCell::new(vec![0.;shape.plane()*4].into_boxed_slice()),drops:drops.clone()});
        let (input,output)=unsafe{(std::ptr::NonNull::new((*owner.input.get()).as_mut_ptr()).unwrap(),std::ptr::NonNull::new((*owner.output.get()).as_mut_ptr()).unwrap())};
        let mut batch=unsafe{Batch::mapped(shape,input,output,owner.clone())};drop(owner);assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed),0);batch.input[0]=0.25;assert_eq!(batch.input[0],0.25);drop(batch);assert_eq!(drops.load(std::sync::atomic::Ordering::Relaxed),1);
    }
    #[test]
    fn batch_memory_is_bounded_for_dense_scenes() {
        assert!(Shape::bounded(256, 2, 1025).is_some());
        assert!(Shape::bounded(256, 4, 1025).is_none());
        assert!(Shape::bounded(257, 1, 1025).is_none());
        assert!(Shape::bounded(16, 5, 1025).is_none());
        assert!(Shape::bounded(0, 1, 1025).is_none());
    }
    #[test]
    fn graph_matches_complex_mac_for_each_partition_count() {
        for parts in 1..=4 {
            let shape = Shape::bounded(8, parts, 17).unwrap();
            let mut b = Batch::new(shape);
            for (i, v) in b.input.iter_mut().enumerate() {
                *v = ((i * 7919 % 1021) as f32 - 510.) / 511.;
            }
            b.scalar_reference();
            let expected = b.output.clone();
            let graph = Graph::new(parts);
            let n = shape.plane();
            let mut values = vec![vec![0.; n]; graph.operands as usize];
            for (i, v) in b.input.chunks_exact(n).enumerate() {
                values[i].copy_from_slice(v);
            }
            for (op, args, out) in &graph.operations {
                for i in 0..n {
                    values[*out as usize][i] = match op {
                        0 => values[args[0] as usize][i] + values[args[1] as usize][i],
                        18 => values[args[0] as usize][i] * values[args[1] as usize][i],
                        36 => values[args[0] as usize][i] - values[args[1] as usize][i],
                        _ => panic!(),
                    };
                }
            }
            for (i, out) in graph.outputs.iter().enumerate() {
                assert_eq!(&expected[i * n..][..n], &values[*out as usize]);
            }
            assert_eq!(graph.operations.len(), 12 * parts + 4 * (parts - 1));
        }
    }
    #[test]
    fn reduced_mantissa_results_cannot_enable_audio_offload() {
        let mut batch = Batch::new(Shape::bounded(2, 2, 17).unwrap());
        for (i, v) in batch.reference.iter_mut().enumerate() {
            *v = ((i * 7919 % 1021) as f32 - 510.) / 511.;
        }
        // Removing 13 mantissa bits approximates the precision loss of a
        // reduced-precision compute path; a framework flag cannot waive it.
        for (out, &value) in batch.output.iter_mut().zip(&batch.reference) {
            *out = f32::from_bits(value.to_bits() & !0x1fff);
        }
        assert!(!batch.reference_matches());
        batch.output.copy_from_slice(&batch.reference);
        assert!(batch.reference_matches());
    }
    #[test]
    fn quiet_and_nonfinite_outputs_cannot_pass_precision_gate() {
        let mut b = Batch::new(Shape::bounded(2, 2, 9).unwrap());
        b.reference.fill(1e-12);
        b.output.fill(0.);
        assert!(!b.reference_matches());
        b.output.copy_from_slice(&b.reference);
        assert!(b.reference_matches());
        b.output[0] = f32::NAN;
        assert!(!b.reference_matches());
        b.output[0] = f32::INFINITY;
        assert!(!b.reference_matches());
    }
    #[test]
    fn actual_convolution_pcm_transition_tail_and_fallback_match() {
        use crate::convolution::StereoPartitionedConvolver;
        for parts in 1..=4 {
            let partition = 16;
            let bins = partition + 1;
            let l: Vec<_> = (0..partition * parts)
                .map(|i| ((i * 17 % 31) as f32 - 15.) * 0.003)
                .collect();
            let rr: Vec<_> = l.iter().rev().map(|x| x * 0.7).collect();
            let mut cpu = StereoPartitionedConvolver::new(&l, &rr, partition).unwrap();
            let mut candidate = StereoPartitionedConvolver::new(&l, &rr, partition).unwrap();
            let mut batch = Batch::new(Shape::bounded(2, parts, bins).unwrap());
            for block in 0..30 {
                if block == 4 || block == 9 || block == 22 {
                    let target: Vec<_> = l.iter().map(|x| -x * 0.4).collect();
                    cpu.transition_to_pair(&rr, &target, 37);
                    candidate.transition_to_pair(&rr, &target, 37);
                }
                let input: Vec<_> = (0..partition)
                    .map(|i| {
                        if block >= 12 {
                            0.
                        } else {
                            (((block * partition + i) * 127 % 997) as f32 - 498.) / 499.
                        }
                    })
                    .collect();
                let mut a = vec![0.; partition];
                let mut ar = a.clone();
                let mut b = a.clone();
                let mut br = a.clone();
                cpu.process_block(&input, &mut a, &mut ar).unwrap();
                candidate.begin_block(&input).unwrap();
                // Exercise CPU fallback on the same already prepared block,
                // including during retargeting and while draining filter tails.
                if block % 3 == 0 {
                    candidate.calculate_spectra();
                } else {
                    batch.clear();
                    candidate.pack_spectra(&mut batch, 0);
                    batch.scalar_reference();
                    candidate.import_spectra(&batch, 0);
                }
                candidate.finish_prepared_block(&input, &mut b, &mut br);
                for (x, y) in a.iter().chain(&ar).zip(b.iter().chain(&br)) {
                    assert!(
                        (x - y).abs() < 2e-7,
                        "parts={parts} block={block}: {x} != {y}"
                    );
                }
            }
        }
    }
    #[test]
    fn reset_discards_a_prepared_block_before_fallback() {
        let mut c=crate::convolution::StereoPartitionedConvolver::new(&[0.25;32],&[0.5;32],16).unwrap();
        c.begin_block(&[1.;16]).unwrap();assert!(c.block_is_prepared());c.reset();
        assert!(!c.block_is_prepared());c.calculate_spectra();
        let mut l=[0.;16];let mut r=[0.;16];c.finish_prepared_block(&[1.;16],&mut l,&mut r);
        assert_eq!(l,[0.;16]);assert_eq!(r,[0.;16]);
    }

}

#[cfg(all(target_os="android",feature="pcm-diagnostic"))]
mod cooperative_probe;
#[cfg(all(target_os="android",feature="pcm-diagnostic"))]
pub fn probe_cooperative_audio(count:usize,constrain:bool)->Result<serde_json::Value,String>{cooperative_probe::run(count,constrain)}
