use super::*;
use sda_format::{BedFile, PreparedResources};
pub(crate) struct Source {
    pub file: BedFile,
    pub _resources: PreparedResources,
    pub scene: sda_native_renderer::authored::Scene,
    pub info: serde_json::Value,
}
impl MobileEngine {
    fn apply_authored_scene(
        &mut self,
        scene: sda_native_renderer::authored::Scene,
    ) -> EngineResult<()> {
        if let Some(pipeline) = &self.pipeline {
            let (reply, received) = std::sync::mpsc::channel();
            pipeline
                .commands
                .push(render_command::RenderCommand::AuthoredScene {
                    scene: Box::new(scene),
                    reply,
                })
                .map_err(|_| "SDAR render command queue full")?;
            received
                .recv_timeout(Duration::from_secs(60))
                .map_err(|_| "SDAR renderer preparation timed out".to_string())??;
        } else {
            self.renderer
                .as_mut()
                .ok_or("renderer unavailable")?
                .configure_authored_scene(scene)?;
        }
        Ok(())
    }
    pub fn open_sdar(&mut self, path: &str, temp_root: Option<&str>) -> EngineResult<u32> {
        if self.sdar_source.is_some()
            || self.mp3_decoder.is_some()
            || self.alac_decoder.is_some()
            || self.mpegh_decoder.is_some()
            || self.decoded_sample_pos() != 0
        {
            return Err("engine already has an active source".into());
        }
        let mut file = BedFile::open_interchange(path)?;
        let resources = if let Some(root) = temp_root {
            PreparedResources::new_in(&file.metadata, std::path::Path::new(root))?
        } else {
            PreparedResources::new(&file.metadata)?
        };
        let value = resources.renderer_scene(&file.metadata, "7.1.4")?;
        let scene: sda_native_renderer::authored::Scene =
            serde_json::from_value(value).map_err(|e| e.to_string())?;
        let info = serde_json::json!({"summary":file.metadata.summary(),"inputRate":file.input_rate,"format":if file.transport.as_ref().is_some_and(|v|v["container"]!="sdar"){"SDA M4A"}else{"SDAR Bed Channel"},"frames":file.frames(),"covers":resources.covers,"lyrics":resources.lyrics});
        self.apply_authored_scene(scene.clone())?;
        self.config.layout = "7.1.4".into();
        self.config.hrtf_wet_weight = scene.resources.hrtf_wet;
        self.config.binaural_enabled = true;
        self.config.direct_object_hrtf = scene.render["playbackSnapshot"]["hrtf"]
            ["directObjectHrtf"]
            .as_bool()
            .unwrap_or(true);
        self.config.directional_hrtf = scene.render["playbackSnapshot"]["native"]
            ["directionalHrtf"]
            .as_bool()
            .unwrap_or(true);
        self.config.object_rendering =
            self.config.direct_object_hrtf || self.config.directional_hrtf;
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&resources.hrtf).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        self.hrtf_directions = manifest["positions"].as_array().map_or(0, Vec::len);
        self.hrtf_loaded = true;
        self.balance.reset();
        self.balance.enabled = false;
        self.stereo_bed_mode = false;
        let rate = file.input_rate;
        file.metadata.assets.clear();
        self.sdar_source = Some(Source {
            file,
            _resources: resources,
            scene,
            info,
        });
        self.announced_codec = None;
        Ok(rate)
    }
    pub fn sdar_info(&self) -> serde_json::Value {
        self.sdar_source
            .as_ref()
            .map_or(serde_json::Value::Null, |source| source.info.clone())
    }
    pub fn pull_sdar(&mut self, max_frames: usize) -> EngineResult<(usize, bool)> {
        let source = self
            .sdar_source
            .as_mut()
            .ok_or("SDAR decoder unavailable")?;
        let source_start=source.file.position();
        let channels = source.file.read(max_frames.clamp(1, 4096))?;
        let count = channels[0].len();
        let done = source.file.finished();
        let labels = source.file.metadata.renderer_labels();
        let mut events:Vec<ObjectEvent>=source.file.objects.events(source_start,count).into_iter().map(|event|serde_json::from_value(serde_json::to_value(event).unwrap()).map_err(|e|e.to_string())).collect::<Result<_,_>>()?;
        let object_channels=source.file.objects.channels.iter().map(|object|sda_core::ObjectChannelDecl{id:object.id,channel:object.channel}).collect();
        if count > 0 {
            let start = self.decoded_sample_pos();
            for event in &mut events {event.sample_pos=start+event.sample_pos.saturating_sub(source_start);}
            if self.announced_codec.as_deref() != Some("sdar") {
                if let Some(pipeline) = &self.pipeline {
                    pipeline
                        .commands
                        .push(render_command::RenderCommand::Command(
                            Command::SetProgramCodec {
                                codec: "sdar".into(),
                            },
                        ))
                        .map_err(|_| "SDAR codec announcement queue full")?;
                }
                self.announced_codec = Some("sdar".into());
            }
            let frame = FrameData {
                reference_loudness: None,
                compatibility_bed: None,
                codec: "sdar",
                sample_rate: 48000,
                sample_pos: start,
                channels,
                labels: labels.clone(),
                raw_bed_labels: labels.into_iter().filter(|label|!label.starts_with("Obj_")).collect(),
                events,
                object_channels,
                program_loudness: None,
                ramp_duration: 0,
            };
            self.pending
                .lock()
                .expect("pending lock")
                .push_back(balance::PendingFrame {
                    frame,
                    measurement: None,
                });
            *self.newest_sample_pos.lock().expect("clock lock") = start + count as u64;
            self.drain_pending_into_pipeline()?;
        }
        Ok((count, done))
    }
    pub fn seek_sdar(&mut self, position_ms: u64) -> EngineResult<()> {
        let scene = {
            let source = self
                .sdar_source
                .as_mut()
                .ok_or("SDAR decoder unavailable")?;
            source.file.seek(position_ms.saturating_mul(48))?;
            source.scene.clone()
        };
        self.pending.lock().expect("pending lock").clear();
        *self.newest_sample_pos.lock().expect("clock lock") = 0;
        *self.last_poll.lock().expect("poll lock") = None;
        self.object_events
            .lock()
            .expect("object events lock")
            .clear();
        self.active_objects
            .lock()
            .expect("active objects lock")
            .clear();
        self.frame_router = frame_router::FrameRouter::default();
        self.balance.reset();
        self.balance.enabled = false;
        self.announced_codec = None;
        *self.startup.lock().expect("startup lock") = StartupGate::default();
        if let Some(pipeline) = &self.pipeline {
            pipeline
                .commands
                .push(render_command::RenderCommand::Command(Command::Reset {
                    origin: 0,
                }))
                .map_err(|_| "SDAR seek queue full")?;
            pipeline.fifo.clear_from_producer();
            pipeline
                .telemetry
                .callback_consumed_sample_pos
                .store(0, std::sync::atomic::Ordering::Release);
        }
        self.apply_authored_scene(scene)?;
        Ok(())
    }
}

#[cfg(test)]mod tests {
 use super::*;
 #[test]
 #[ignore="requires generated object export fixtures; set SDA_OBJECT_FIXTURE_DIR"]
 fn object_interchange_preserves_mobile_pcm_routes_and_seek_clock(){
  let root=std::env::var("SDA_OBJECT_FIXTURE_DIR").expect("SDA_OBJECT_FIXTURE_DIR");
  for name in ["master.wav","objects.sdar","objects.m4a"]{
   let path=std::path::Path::new(&root).join(name);
   assert!(BedFile::is_sda(&path),"authored content probe must route {name} through the SDA scene decoder");
   let mut engine=MobileEngine::new(EngineConfig::default(),None).unwrap();
   assert_eq!(engine.open_sdar(path.to_str().unwrap(),None).unwrap(),48000);
   assert!(!engine.balance.enabled);assert!(!engine.stereo_bed_mode);
   let (count,done)=engine.pull_sdar(4096).unwrap();assert_eq!(count,4096);assert!(!done);
   let mut frames=engine.take_pending_frames();assert_eq!(frames.len(),1);let frame=frames.remove(0);
   assert_eq!(frame.channels.len(),66);assert_eq!(frame.object_channels.len(),64);assert_eq!(frame.events.len(),64);
   assert_eq!(frame.events[0].size,[0.4,0.2,0.3]);assert_eq!(frame.events[0].diffuse,0.1);
   let mut router=frame_router::FrameRouter::default();let commands=router.route(frame).unwrap();
   let render_command::RenderCommand::PcmFrame{entries,events,..}=commands.last().unwrap() else{panic!("missing object PCM frame")};
   assert_eq!(entries[0].0,"bed:0");assert_eq!(entries[1].0,"obj:1");assert_eq!(entries[19].0,"bed:19");assert_eq!(events.len(),64);
   engine.seek_sdar(150).unwrap();engine.pull_sdar(733).unwrap();let frames=engine.take_pending_frames();assert_eq!(frames[0].sample_pos,0);
   let recovered=frames[0].events.iter().filter(|e|e.id==1).collect::<Vec<_>>();assert_eq!(recovered.len(),2);
   assert_eq!(recovered[0].sample_pos,0);assert!(recovered[0].pos[0].abs()<1e-12);assert_eq!(recovered[1].ramp_duration,2400);
   assert_eq!(recovered[1].sample_pos,0);
  }
 }
}
