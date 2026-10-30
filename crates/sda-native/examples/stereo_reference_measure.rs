//! Measure a new complete stereo reference without constructing a renderer.
use sda_native::stereo_reference::FreshStereoReference;
fn main()->Result<(),Box<dyn std::error::Error>> {
    let args:Vec<_>=std::env::args().collect();if args.len()!=4 {return Err("INPUT alac|mp3 OUTPUT.json".into());}
    let began=std::time::Instant::now();let mut reference=FreshStereoReference::open(&args[1],args[2]=="alac")?;let mut worst=0.;
    let result=loop {let at=std::time::Instant::now();let step=reference.step(65536)?;worst=f64::max(worst,at.elapsed().as_secs_f64());if step.done {break step.measurement.unwrap();}};
    std::fs::write(&args[3],serde_json::to_string_pretty(&result)?)?;
    println!("{}",serde_json::json!({"measurement":result,"elapsedSeconds":began.elapsed().as_secs_f64(),"maximumStepSeconds":worst}));Ok(())
}
