//! Offline 7.1.4 source-domain balances, sharing the live directional kernel.
use crate::{Source,SourceKind,auxiliary_balance,convolution::DEFAULT_PARTITION,dsp,vbap};
use std::{fs::OpenOptions,io::{Read,Write,BufReader,BufWriter},sync::{Arc,atomic::{AtomicBool,Ordering}},time::Instant};
const LABELS:[&str;12]=["L","R","C","LFE","Ls","Rs","Lb","Rb","Tfl","Tfr","Trl","Trr"];
pub struct BedProcessor {
    sources:[Source;12],solver:vbap::VbapSolver,control:auxiliary_balance::AuxiliaryBalance,
    shelf:[dsp::Biquad;12],at:u64,spatial:bool,programme:f32,output:f32,
}
impl BedProcessor {
    pub fn new(programme_db:f32,makeup_db:f32,spatial:bool)->Result<Self,String>{
        if !programme_db.is_finite()||!(-60.0..=0.0).contains(&programme_db)||!matches!(makeup_db,0.0|2.5){return Err("invalid offline programme gain".into());}
        let mut control=auxiliary_balance::AuxiliaryBalance::directional_detail(48000);control.set_enabled(spatial,true,0);
        let shelf=dsp::Biquad::highshelf(48000,4000.,std::f32::consts::FRAC_1_SQRT_2,0.6)?;
        Ok(Self{sources:std::array::from_fn(|i|Source{kind:SourceKind::Bed,bed_label:Some(LABELS[i].into()),..Source::default()}),solver:vbap::VbapSolver::new(),control,shelf:[shelf;12],at:0,spatial,programme:10f32.powf(programme_db/20.),output:10f32.powf((programme_db+makeup_db)/20.)})
    }
    pub fn process(&mut self,input:[f32;12])->Result<[f32;12],String>{
        if input.iter().any(|x|!x.is_finite()){return Err("non-finite bed input".into());}
        let index=self.at as usize%DEFAULT_PARTITION;
        let mut reference=auxiliary_balance::Reference::default();
        let mut output=[0.;12];
        for ch in 0..12 {
            let x=if self.spatial {auxiliary_balance::process_source_detail(&mut self.sources[ch],input[ch],self.at,self.control.gains[index],&mut reference,48000,&self.solver,Some(self.control.temporal_control()),index)}else{input[ch]};
            output[ch]=if self.spatial {self.shelf[ch].process(x)*self.output}else{x*self.output};
        }
        if self.spatial {self.control.observe_at(reference,self.programme,self.at);if index+1==DEFAULT_PARTITION {self.control.finish_block();}}
        self.at+=1;Ok(output)
    }
}
fn render(args:&[String])->Result<(),String>{
    if args.len()!=7{return Err("expected input, output, programme dB, makeup dB, spatial flag".into());}
    let parse=|i:usize|args[i].parse::<f32>().map_err(|_|"invalid gain".to_string());
    let spatial=match args[6].as_str(){"true"=>true,"false"=>false,_=>return Err("invalid spatial flag".into())};
    let mut processor=BedProcessor::new(parse(4)?,parse(5)?,spatial)?;
    println!("{}",serde_json::json!({"type":"bedExportReady","protocol":1,"algorithm":"directional-spectral-redistribution-v1"}));
    let input=std::fs::File::open(&args[2]).map_err(|e|e.to_string())?;
    let bytes=input.metadata().map_err(|e|e.to_string())?.len();if bytes==0||bytes%48!=0{return Err("invalid 12-channel float spool".into());}
    let mut input=BufReader::new(input);let mut output=BufWriter::new(OpenOptions::new().write(true).create_new(true).open(&args[3]).map_err(|e|e.to_string())?);
    let cancel=Arc::new(AtomicBool::new(false));let signal=cancel.clone();
    std::thread::spawn(move||{let mut byte=[0u8;1];let _=std::io::stdin().read(&mut byte);signal.store(true,Ordering::Release);});
    let mut read=0u64;let mut tick=Instant::now();let mut buffer=vec![0u8;4096*48];
    while read<bytes {
        if cancel.load(Ordering::Acquire){return Err("EXPORT_CANCELLED".into());}
        let length=((bytes-read) as usize).min(buffer.len());input.read_exact(&mut buffer[..length]).map_err(|e|e.to_string())?;
        for frame in buffer[..length].chunks_exact_mut(48){
            let values=std::array::from_fn(|ch|f32::from_le_bytes(frame[ch*4..ch*4+4].try_into().unwrap()));
            for (ch,value) in processor.process(values)?.iter().enumerate(){frame[ch*4..ch*4+4].copy_from_slice(&value.to_le_bytes());}
        }
        output.write_all(&buffer[..length]).map_err(|e|e.to_string())?;read+=length as u64;
        if tick.elapsed().as_millis()>=200||read==bytes {println!("{}",serde_json::json!({"processedFrames":read/48,"frames":bytes/48}));tick=Instant::now();}
    }
    output.flush().map_err(|e|e.to_string())?;output.get_ref().sync_all().map_err(|e|e.to_string())?;Ok(())
}
pub fn entry()->bool {
    let args:Vec<_>=std::env::args().collect();if args.get(1).map(String::as_str)!=Some("--sda-export-upmix-bed"){return false;}
    if let Err(error)=render(&args){eprintln!("{error}");std::process::exit(1);}true
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_spatial_is_exact_and_programme_gain_is_linked(){
        let mut dry=BedProcessor::new(0.,0.,false).unwrap();let mut gain=BedProcessor::new(-6.,2.5,false).unwrap();
        for i in 0..12000 {let input=std::array::from_fn(|ch|((i+ch*5)as f32*0.13).sin()*0.2);assert_eq!(dry.process(input).unwrap(),input);let actual=gain.process(input).unwrap();for ch in 0..12 {assert_eq!(actual[ch],input[ch]*10f32.powf(-3.5/20.));}}
    }
    #[test]
    fn directional_source_kernel_protects_front_and_lfe_and_enhances_rear_detail(){
        let mut p=BedProcessor::new(0.,0.,true).unwrap();
        let mut front=dsp::Biquad::highshelf(48000,4000.,std::f32::consts::FRAC_1_SQRT_2,0.6).unwrap();let mut lfe=front;
        let mut rear_delta=0.;let mut n=0.;
        for i in 0..48000*3 {
            let x=|hz:f64,a:f64|(std::f64::consts::TAU*hz*i as f64/48000.).sin()as f32*a as f32;
            let mut input=[0.;12];input[0]=x(700.,0.2);input[1]=x(800.,0.2);input[2]=x(500.,0.1);input[3]=x(80.,0.05);input[6]=x(80.,0.015)+x(400.,0.01)+x(3000.,0.004);input[7]=input[6];
            let out=p.process(input).unwrap();assert_eq!(out[0],front.process(input[0]));assert_eq!(out[3],lfe.process(input[3]));if i>48000*2 {rear_delta+=f64::from((out[6]-input[6]).abs());n+=1.;}
            assert!(out.iter().all(|x|x.is_finite()));
        }
        assert!(rear_delta/n>1e-4,"rear detail was not processed");
    }
}
