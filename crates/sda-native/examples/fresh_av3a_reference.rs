fn main()->Result<(),Box<dyn std::error::Error>> {
 let path=std::env::args().nth(1).ok_or("INPUT.av3a")?;
 let hrtf=std::env::args().nth(2);
 let mut source=if let Some(hrtf)=hrtf{sda_native::av3a_reference::open_with_hrtf(path,hrtf)?}else{sda_native::av3a_reference::open(path)?};
 loop {let step=source.step(65536)?;if step.done {if let Some(diagnostics)=&step.diagnostics {eprintln!("{}",serde_json::to_string(diagnostics)?);}println!("{}",serde_json::to_string_pretty(&step.measurement.unwrap())?);break;}}
 Ok(())
}
