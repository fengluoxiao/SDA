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
        let channels = source.file.read(max_frames.clamp(1, 4096))?;
        let count = channels[0].len();
        let done = source.file.finished();
        let labels = source.file.metadata.renderer_labels();
        if count > 0 {
            let start = self.decoded_sample_pos();
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
                raw_bed_labels: labels,
                events: vec![],
                object_channels: vec![],
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
