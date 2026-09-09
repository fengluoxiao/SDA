//! Generic electrical model at 48 kHz. Voltages refer to a resistive load,
//! not acoustic SPL. This is not a measured model of a commercial device.
use serde::Deserialize;
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all="camelCase", default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool, pub input_db: f32, pub dac_bits: u32,
    pub line_rms: f32, pub gain_db: f32, pub rail_v: f32,
    pub current_a: f32, pub load_ohms: f32, pub output_ohms: f32, pub bandwidth_hz: f32,
}
impl Default for Settings {
    fn default() -> Self { Self { enabled:false, input_db:0.0, dac_bits:24,
        line_rms:2.0, gain_db:26.0, rail_v:28.0, current_a:7.0,
        load_ohms:8.0, output_ohms:0.05, bandwidth_hz:60000.0 } }
}
impl Settings {
    pub fn validate(&self) -> Result<(),String> {
        for (v,lo,hi) in [(self.input_db,-60.0,12.0),(self.line_rms,0.1,12.0),
            (self.gain_db,0.0,40.0),(self.rail_v,1.0,80.0),(self.current_a,0.01,30.0),
            (self.load_ohms,2.0,600.0),(self.output_ohms,0.0,20.0),(self.bandwidth_hz,5000.0,250000.0)] {
            if !v.is_finite() || v<lo || v>hi { return Err("invalid hardware parameter".into()); }
        }
        if !(8..=24).contains(&self.dac_bits) { return Err("invalid DAC precision".into()); }
        Ok(())
    }
}
const TAPS:usize=33;
pub struct Chain {
    enabled:bool, drive:f32, steps:f32, volts:f32, limit:f32, divider:f32, alpha:f32,
    taps:[f32;TAPS], up:[f32;TAPS], down:[f32;TAPS], cursor:usize, pole:f32,
}
impl Chain {
    pub fn new(s:&Settings) -> Self {
        let mut taps=[0.0;TAPS];
        for (i,t) in taps.iter_mut().enumerate() {
            let x=i as f32-16.0;
            let sinc=if x==0.0 {0.225} else {(std::f32::consts::PI*0.225*x).sin()/(std::f32::consts::PI*x)};
            let phase=2.0*std::f32::consts::PI*i as f32/32.0;
            *t=sinc*(0.42-0.5*phase.cos()+0.08*(2.0*phase).cos());
        }
        let sum:f32=taps.iter().sum();
        for t in &mut taps {*t/=sum;}
        Self {enabled:s.enabled, drive:10.0_f32.powf(s.input_db/20.0),steps:(1u32<<(s.dac_bits-1)) as f32,
            volts:s.line_rms*2.0_f32.sqrt()*10.0_f32.powf(s.gain_db/20.0),
            limit:s.rail_v.min(s.current_a*(s.load_ohms+s.output_ohms)),
            divider:s.load_ohms/(s.load_ohms+s.output_ohms),
            alpha:1.0-(-2.0*std::f32::consts::PI*s.bandwidth_hz/192000.0).exp(),
            taps,up:[0.0;TAPS],down:[0.0;TAPS],cursor:0,pole:0.0}
    }
    pub fn reset(&mut self) {self.up.fill(0.0);self.down.fill(0.0);self.cursor=0;self.pole=0.0;}
    pub fn process(&mut self,input:f32)->f32 {
        if !self.enabled {return input;}
        let quantized=((input*self.drive).clamp(-1.0,1.0-1.0/self.steps)*self.steps).round()/self.steps;
        let mut output=0.0;
        // Zero-stuff and low-pass before the nonlinear stage; filter again before
        // decimation. Both FIRs together add eight 48 kHz samples of delay.
        for phase in 0..4 {
            self.up[self.cursor]=if phase==0 {quantized*4.0} else {0.0};
            let mut reconstructed=0.0;
            for i in 0..TAPS {reconstructed+=self.taps[i]*self.up[(self.cursor+TAPS-i)%TAPS];}
            self.pole+=self.alpha*(reconstructed*self.volts-self.pole);
            self.down[self.cursor]=self.pole.clamp(-self.limit,self.limit)*self.divider/self.volts;
            if phase==0 {for i in 0..TAPS {output+=self.taps[i]*self.down[(self.cursor+TAPS-i)%TAPS];}}
            self.cursor=(self.cursor+1)%TAPS;
        }
        output
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn ahb2_gain_modes_preserve_normalized_level() {
        for (line, gain) in [(2.0,23.0),(4.0,17.0),(9.8,9.2)] {
            let s=Settings { enabled:true, input_db:0.0, line_rms:line, gain_db:gain,
                rail_v:40.0*(1.0+1.0/254.0), current_a:29.0, load_ohms:8.0,
                output_ohms:8.0/254.0, bandwidth_hz:200000.0, ..Settings::default() };
            s.validate().unwrap();
            let mut c=Chain::new(&s);
            let mut energy=0.0_f64;
            let mut count=0;
            for n in 0..12000 {
                let y=c.process(0.5*(2.0*std::f32::consts::PI*1000.0*n as f32/48000.0).sin());
                assert!(y.is_finite());
                if n>=1000 {energy+=(y as f64).powi(2);count+=1;}
            }
            let ratio=(energy/count as f64).sqrt()/(0.5/2.0_f64.sqrt());
            assert!((20.0*ratio.log10()).abs()<0.15,"unexpected attenuation: {ratio}");
            assert!((c.limit*c.divider-40.0).abs()<1e-4);
        }
    }
    #[test] fn exact_bypass_and_reset() {
        let mut c=Chain::new(&Settings::default());
        for x in [-2.0,0.0,0.123,2.0] {assert_eq!(c.process(x),x);}
        let mut s=Settings::default();s.enabled=true;
        let mut c=Chain::new(&s);for _ in 0..100 {c.process(0.5);}
        c.reset();for _ in 0..100 {assert_eq!(c.process(0.0),0.0);}
    }
    #[test] fn resistive_load_and_current_limit() {
        let mut s=Settings::default();s.enabled=true;s.input_db=0.0;s.current_a=0.01;
        let mut c=Chain::new(&s);let mut y=0.0;
        for _ in 0..2000 {y=c.process(0.5);}
        let expected=s.current_a*s.load_ohms/c.volts;
        assert!((y-expected).abs()<1e-6);
        s.current_a=30.0;s.rail_v=1.0;
        let mut c=Chain::new(&s);for _ in 0..2000 {y=c.process(-0.5);}
        assert!((y+s.rail_v*c.divider/c.volts).abs()<1e-6);
    }
    #[test] fn low_level_gain_and_invalid_values() {
        let mut s=Settings::default();s.enabled=true;
        let mut c=Chain::new(&s);let mut energy=0.0;
        for n in 0..48000 {let y=c.process(0.01*(2.0*std::f32::consts::PI*1000.0*n as f32/48000.0).sin());if n>1000 {energy+=y*y;}}
        let rms=(energy/46999.0_f32).sqrt();
        assert!((rms/(0.01/2.0_f32.sqrt()*c.drive*c.divider)-1.0).abs()<0.02);
        s.rail_v=f32::NAN;assert!(s.validate().is_err());
    }
}
