//! Experimental normalized source footprint and decorrelated diffuse component.
//! Width/height map to at most 120 degrees; depth is a normalized local extent,
//! not metres. No semantic source separation and no added room reverberation.
use crate::{vbap,spatial,adm_zone};
#[derive(Clone,Copy,Debug,Default,serde::Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Settings { pub enabled:bool, pub width:f32, pub diffusion:f32 }
impl Settings {
    pub fn valid(&self)->bool { [self.width,self.diffusion].iter().all(|v|v.is_finite()&&(0.0..=1.0).contains(v)) }
}
pub fn route(solver:&vbap::VbapSolver,position:[f32;3],head:Option<[f32;4]>,size:[f32;3],horizontal:bool,zones:&[adm_zone::Zone],settings:Settings)->[f32;vbap::MAX_BUS_COUNT] {
    let s=spatial::adm_to_spherical(position);
    let width=size[0].max(settings.width).clamp(0.0,1.0)*120.0;
    let height=if horizontal{0.0}else{size[2].clamp(0.0,1.0)*120.0};
    let depth=size[1].clamp(0.0,1.0);
    if width==0.0&&height==0.0 {
        let p=spatial::head_relative_adm(position,head);
        let mut out=if horizontal{solver.pan_horizontal(p,0.0)}else{solver.pan(p,0.0)};
        adm_zone::apply(&mut out,solver,zones);return out;
    }
    let mut energy=[0.0;vbap::MAX_BUS_COUNT];
    // Symmetric quadrature around the authored centre. Mix power, not coherent
    // copies of multiple sample points; a single dry signal reaches each bus.
    for (da,de,radial,weight) in [(0.0,0.0,1.0,1.0/3.0),(-0.5,0.0,1.0,1.0/12.0),(0.5,0.0,1.0,1.0/12.0),(0.0,-0.5,1.0,1.0/12.0),(0.0,0.5,1.0,1.0/12.0),(-0.5,0.0,1.0-depth*0.5,1.0/12.0),(0.5,0.0,1.0-depth*0.5,1.0/12.0),(-0.5,0.0,1.0+depth*0.5,1.0/12.0),(0.5,0.0,1.0+depth*0.5,1.0/12.0)] {
        // Depth adjusts apparent angular footprint, without inventing attenuation.
        let az=(s.azimuth+da*width/radial.max(0.5)).to_radians();
        let el=(s.elevation+de*height/radial.max(0.5)).clamp(-89.9,89.9).to_radians();
        let p=spatial::head_relative_adm([-az.sin()*el.cos(),az.cos()*el.cos(),el.sin()],head);
        let gains=if horizontal{solver.pan_horizontal(p,0.0)}else{solver.pan(p,0.0)};
        for i in 0..solver.bus_count(){energy[i]+=weight*gains[i]*gains[i];}
    }
    let mut out=energy.map(f32::sqrt);adm_zone::apply(&mut out,solver,zones);out
}
struct Allpass { data:Vec<f32>,cursor:usize }
impl Allpass {
    fn process(&mut self,x:f32)->f32 {let delayed=self.data[self.cursor];let y=delayed-0.5*x;self.data[self.cursor]=x+0.5*y;self.cursor+=1;if self.cursor==self.data.len(){self.cursor=0;}y}
}
pub struct Diffuser { filters:Vec<Allpass> }
impl Diffuser {
    pub fn new()->Self {Self{filters:(0..vbap::MAX_BUS_COUNT).map(|i|Allpass{data:vec![0.0;149+i*46],cursor:0}).collect()}}
    #[cfg(test)]
    pub fn process(&mut self,x:f32)->[f32;vbap::MAX_BUS_COUNT] {self.process_inputs([x;vbap::MAX_BUS_COUNT])}
    pub fn process_inputs(&mut self,x:[f32;vbap::MAX_BUS_COUNT])->[f32;vbap::MAX_BUS_COUNT] {std::array::from_fn(|i|self.filters[i].process(x[i]))}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore="offline performance measurement"]
    fn benchmark_extent_128_sources(){
        let solver=vbap::VbapSolver::with_layout(vbap::LayoutId::Dolby9_1_6);
        let settings=Settings{enabled:true,width:0.5,diffusion:0.25};
        let mut filter=Diffuser::new();
        let start=std::time::Instant::now();let mut checksum=0.0;
        for i in 0..4800 {let mut input=[0.0;vbap::MAX_BUS_COUNT];for j in 0..128 {
            for x in &mut input{*x+=std::hint::black_box(0.01);}
            if i%512==0 {let angle=(i+j) as f32*0.013;std::hint::black_box(route(&solver,[angle.sin(),angle.cos(),0.5],None,[0.5;3],false,&[],settings));}
        }checksum+=filter.process_inputs(input)[0];}
        eprintln!("128 moving sources extent/diffusion overhead per 100ms: {:.2}ms checksum={checksum}",start.elapsed().as_secs_f64()*1000.0);
    }
    #[test] fn extent_preserves_zero_and_width_height_are_distinct(){
        let solver=vbap::VbapSolver::new();let settings=Settings{enabled:true,..Default::default()};
        let point=route(&solver,[0.0,1.0,0.0],None,[0.0;3],false,&[],settings);
        let original=solver.pan([0.0,1.0,0.0],0.0);
        for i in 0..solver.bus_count(){assert!((point[i]-original[i]).abs()<1e-6);}
        let wide=route(&solver,[0.0,1.0,0.0],None,[0.8,0.0,0.0],false,&[],settings);
        let tall=route(&solver,[0.0,1.0,0.0],None,[0.0,0.0,0.8],false,&[],settings);
        assert_ne!(wide,tall);
        for gains in [point,wide,tall]{assert!((gains.iter().map(|g|g*g).sum::<f32>()-1.0).abs()<1e-5);}
        assert!(wide[0]>0.0&&wide[1]>0.0);assert!((wide[0]-wide[1]).abs()<1e-5);
    }
    #[test] fn diffuser_is_energy_preserving_and_channels_differ(){
        let mut d=Diffuser::new();let mut energy=[0.0;vbap::MAX_BUS_COUNT];let mut cross=0.0;
        for i in 0..48000 {let out=d.process(if i==0{1.0}else{0.0});for b in 0..energy.len(){assert!(out[b].is_finite());energy[b]+=out[b]*out[b];}cross+=out[0]*out[1];}
        assert!(energy.iter().all(|e|(e-1.0).abs()<1e-5));assert!(cross.abs()<0.3);
    }
    #[test] fn shared_diffusion_matches_independent_linear_sources(){
        let mut a=Diffuser::new();let mut b=Diffuser::new();let mut combined=Diffuser::new();
        for i in 0..4800 {
            let x=(i as f32*0.173).sin()*0.1;let y=(i as f32*0.217).cos()*0.05;
            let one=a.process(x);let two=b.process(y);let sum=combined.process(x+y);
            for bus in 0..vbap::MAX_BUS_COUNT{assert!((one[bus]+two[bus]-sum[bus]).abs()<2e-7);}
        }
    }
    #[test] fn extent_changes_actual_pcm_and_zero_matches_bypass(){
        let render=|settings:Settings,direct:bool|{
            let mut e=crate::Engine::new(48000,2);
            let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../web/public/hrtf/hrtf-set.json");
            e.replace_hrtf(crate::hrtf::NativeHrtfSet::load_calibrated(&path).unwrap(),0.0).unwrap();
            e.source_extent=settings;e.paused=false;e.output_active=true;
            let mut source=crate::Source{kind:crate::SourceKind::Object,position:[0.0,1.0,0.0],gain:1.0,target_gain:1.0,availability:1.0,availability_target:1.0,..Default::default()};
            let mut seed=1u32;let samples:Vec<f32>=(0..24000).map(|_|{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);((seed>>8) as f32/16777216.0-0.5)*0.01}).collect();
            source.samples.write(0,0,&samples);e.sources.insert("obj:test".into(),source);e.route_source_now("obj:test",0).unwrap();e.set_direct_objects(direct).unwrap();
            let mut out=vec![0.0;48000];e.render_into(&mut out,2);assert!(out.iter().all(|x|x.is_finite()));out
        };
        for direct in [false,true]{
            let bypass=render(Settings::default(),direct);
            let point=render(Settings{enabled:true,..Default::default()},direct);
            assert_eq!(bypass,point,"zero extent/diffusion must be exact bypass");
            let spread=render(Settings{enabled:true,width:0.5,diffusion:0.25},direct);
            let energy:f32=point[24000..].iter().map(|x|x*x).sum();
            let changed:f32=spread[24000..].iter().zip(&point[24000..]).map(|(a,b)|(a-b)*(a-b)).sum();
            let output_energy:f32=spread[24000..].iter().map(|x|x*x).sum();
            assert!(changed/energy>0.01);assert!((0.1..10.0).contains(&(output_energy/energy)));
            eprintln!("extent PCM direct={direct} energy_ratio={:.3} difference_ratio={:.3}",output_energy/energy,changed/energy);
        }
    }
}
