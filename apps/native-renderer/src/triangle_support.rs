//! Explicit experimental asset support, loaded once; never enabled by count.
use serde::Deserialize;
use std::{path::Path, sync::Arc};
use crate::hrtf::StereoIr;

#[derive(Deserialize)]
struct Point { azimuth: f64, elevation: f64 }
#[derive(Deserialize)]
struct Face { indices: [usize;3], inverse: [[f64;3];3] }
#[derive(Deserialize)]
struct Data { positions: Vec<Point>, faces: Vec<Face> }
pub(crate) struct TriangleSupport { faces: Vec<Face>, directions: Vec<[f64;3]>, mirrors: Vec<usize> }
fn unit(a:f64,e:f64)->[f64;3] { let (a,e)=(a.to_radians(),e.to_radians());[-a.sin()*e.cos(),a.cos()*e.cos(),e.sin()] }
impl TriangleSupport {
    pub(crate) fn load(path:&Path,irs:&[StereoIr])->Result<Arc<Self>,String> {
        let bytes=std::fs::read(path).map_err(|e|e.to_string())?;
        if bytes.len()>4*1024*1024 {return Err("Triangle support file too large".into());}
        let data:Data=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
        if data.positions.len()!=irs.len() || data.faces.is_empty() || data.faces.len()>8192 {return Err("Invalid triangle dimensions".into());}
        let mut directions=Vec::new();
        for (p,ir) in data.positions.iter().zip(irs) {
            if !p.azimuth.is_finite() || !p.elevation.is_finite() || (p.azimuth-ir.azimuth).abs()>1e-9 || (p.elevation-ir.elevation).abs()>1e-9 {
                return Err("Triangle support direction mismatch".into());
            }
            directions.push(unit(p.azimuth,p.elevation));
        }
        for face in &data.faces {
            if face.indices.iter().any(|i|*i>=irs.len()) || face.inverse.iter().flatten().any(|v|!v.is_finite()) {return Err("Invalid triangle face".into());}
            for i in 0..3 {for j in 0..3 {
                let value:f64=face.inverse[i].iter().zip(directions[face.indices[j]]).map(|(a,b)|a*b).sum();
                if (value-if i==j {1.0}else{0.0}).abs()>1e-7 {return Err("Triangle inverse mismatch".into());}
            }}
        }
        let mirrors=directions.iter().map(|p|directions.iter().position(|q|(q[0]+p[0]).abs()<1e-9&&(q[1]-p[1]).abs()<1e-9&&(q[2]-p[2]).abs()<1e-9)
            .ok_or_else(||"Triangle support lacks mirror".to_string())).collect::<Result<Vec<_>,_>>()?;
        let support=Self {faces:data.faces,directions,mirrors};
        // Fail at load, not playback, for a visibly incomplete sphere.
        for el in (-90..=90).step_by(15) {for az in (-180..180).step_by(15) {
            if support.solve(unit(az as f64,el as f64)).is_none() {return Err("Triangle hull has uncovered directions".into());}
        }}
        Ok(Arc::new(support))
    }
    fn solve(&self,u:[f64;3])->Option<([usize;3],[f64;3])> {
        for face in &self.faces {
            let mut w:[f64;3]=std::array::from_fn(|i|face.inverse[i].iter().zip(u).map(|(a,b)|a*b).sum());
            if w.iter().all(|v|*v>=-1e-9) {
                for v in &mut w {*v=v.max(0.0);}
                let sum:f64=w.iter().sum();if sum<=0.0 {continue;}
                for v in &mut w {*v/=sum;}
                return Some((face.indices,w));
            }
        }
        None
    }
    pub(crate) fn weights(&self,u:[f64;3])->Option<Vec<(usize,f64)>> {
        if let Some(i)=self.directions.iter().position(|p|p.iter().zip(u).map(|(a,b)|(a-b).powi(2)).sum::<f64>()<1e-18) {return Some(vec![(i,1.0)]);}
        let (a,wa)=self.solve(u)?;let (b,wb)=self.solve([-u[0],u[1],u[2]])?;
        let mut result:Vec<(usize,f64)>=Vec::with_capacity(6);
        for (i,w) in a.into_iter().zip(wa).chain(b.map(|i|self.mirrors[i]).into_iter().zip(wb)) {
            if w<=2e-12 {continue;}
            if let Some(r)=result.iter_mut().find(|r|r.0==i) {r.1+=w*0.5;} else {result.push((i,w*0.5));}
        }
        result.sort_by_key(|r|r.0);Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packaged_combined_grid_preserves_anchors_and_mirrored_weights() {
        let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("../mobile/ios-experiments/fullheight-combined/hrtf-dense");
        let data:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("hrtf-set.json")).unwrap()).unwrap();
        let irs:Vec<_>=data["positions"].as_array().unwrap().iter().map(|p|StereoIr {
            azimuth:p["azimuth"].as_f64().unwrap(),elevation:p["elevation"].as_f64().unwrap(),dry:vec![],wet:vec![]
        }).collect();
        let support=TriangleSupport::load(&root.join("triangle-grid.json"),&irs).unwrap();
        for (i,ir) in irs.iter().enumerate() {assert_eq!(support.weights(unit(ir.azimuth,ir.elevation)).unwrap(),vec![(i,1.0)]);}
        for el in (-75..=75).step_by(15) {for az in (-175..180).step_by(10) {
            let a=support.weights(unit(az as f64,el as f64)).unwrap();
            let b=support.weights(unit(-az as f64,el as f64)).unwrap();
            assert!((a.iter().map(|x|x.1).sum::<f64>()-1.0).abs()<1e-9);
            for (i,w) in a {let other=b.iter().find(|x|x.0==support.mirrors[i]).map_or(0.0,|x|x.1);assert!((w-other).abs()<1e-8);}
        }}
        let mut wrong=irs.clone();wrong[0].azimuth+=1.0;
        assert!(TriangleSupport::load(&root.join("triangle-grid.json"),&wrong).is_err());
    }
}
