//! Windows packages/player/src/bs1770.ts and player.ts playback policy.
//! Measure the stereo master / verified speaker beds / MPEG-H reference, never raw objects
//! or the HRTF output. Gain uses the shared renderer's sample-clock envelope.
use sda_core::FrameData;
use sda_native_renderer::Command;

pub const MIN_BLOCKS: usize = 57;
/// Shared stereo / Audio Vivid decoded preroll; silence must not hold playback forever.
pub const AV3A_STARTUP_MAX_SECONDS: u64 = 7;
pub use sda_core::loudness::{LoudnessMeter, ReferenceLoudness as Measurement};
pub fn master_gain(lufs: f64, peak: Option<f64>) -> f64 {
    if !lufs.is_finite() {
        return 0.0;
    }
    let target = (-18.0 - lufs).clamp(-60.0, 60.0).min(0.0);
    peak.filter(|v| v.is_finite())
        .map_or(target, |v| target.min(-1.0 - v))
}
pub fn stereo_master(f: &FrameData) -> bool {
    f.codec != "adm"
        && f.channels.len() == 2
        && f.labels.len() == 2
        && f.object_channels.is_empty()
        && f.events.is_empty()
        && ["L", "Left", "FrontLeft"].contains(&f.labels[0].as_str())
        && ["R", "Right", "FrontRight"].contains(&f.labels[1].as_str())
}
pub(crate) fn av3a_bed(frame: &FrameData) -> bool {
    frame.codec == "av3a" && frame.sample_rate == 48000 && frame.channels.len() == 12
        && frame.labels == ["L","R","C","LFE","Lb","Rb","Ls","Rs","Tfl","Tfr","Tbl","Tbr"]
        && frame.events.is_empty() && frame.object_channels.is_empty()
}
/// Gate only verified masters, never a stereo core of an object programme.
pub(crate) fn startup_program(frame: &FrameData) -> bool {
    frame.reference_loudness.is_some() || av3a_bed(frame) || (frame.codec != "mpegh" && (stereo_master(frame) || crate::alac_pcm::is_upmixed(frame)))
}
pub struct PendingFrame {
    pub frame: FrameData,
    pub measurement: Option<Measurement>,
}
#[derive(Default)]
pub struct VolumeBalance {
    pub enabled: bool,
    pub eligible: bool,
    non_stereo_seen: bool,
    meter: Option<LoudnessMeter>,
    decoded_reference: Option<Measurement>,
    post_counter: usize,
    cached: Option<Measurement>,
    settled: bool,
    balanced_blocks: usize,
    pub gain_db: f64,
    completed: bool,
    early_alac_balance: bool,
}
impl VolumeBalance {
    /// Only the Apple-decoded ALAC route opts in. Other codec scheduling and
    /// full-track cache validity keep the established desktop policy.
    pub fn enable_alac_startup(&mut self) { self.early_alac_balance = true; }
    pub fn reset(&mut self) {
        let enabled = self.enabled;
        *self = Self {
            enabled,
            ..Default::default()
        };
    }
    pub fn set_cached(&mut self, cached: Measurement) {
        if self.decoded_reference.is_some() {return;}
        self.cached = if cached.blocks >= MIN_BLOCKS
            && cached.integrated_lufs.is_some_and(|v| v.is_finite())
        {
            Some(cached)
        } else {
            None
        };
        self.settled = false;
    }
    pub fn measure(&mut self, frame: FrameData, reference: Option<Vec<Vec<f32>>>) -> PendingFrame {
        let channels = if frame.codec == "mpegh" || crate::alac_pcm::is_upmixed(&frame) {
            reference.as_deref()
        } else if stereo_master(&frame) || av3a_bed(&frame) {
            Some(frame.channels.as_slice())
        } else {
            None
        };
        let mut measurement = frame.reference_loudness.clone();
        if measurement.is_some() {
            if self.decoded_reference.is_none() {self.cached=None;self.settled=false;self.meter=None;self.post_counter=0;}
            self.decoded_reference = measurement.clone();
        }
        if let Some(channels) = channels.filter(|_| measurement.is_none()) {
            self.meter
                .get_or_insert_with(|| if av3a_bed(&frame) { LoudnessMeter::speakers_7_1_4() } else { LoudnessMeter::new(frame.sample_rate) })
                .push(channels);
            self.post_counter += 1;
            if self.post_counter % 8 == 0 {
                measurement = self.meter.as_ref().map(LoudnessMeter::integrated);
            }
        }
        PendingFrame { frame, measurement }
    }
    pub fn startup_measurement_ready(&self) -> bool {
        self.cached.is_some() || self.decoded_reference.as_ref().is_some_and(|m|m.blocks>=MIN_BLOCKS) || self.meter.as_ref().is_some_and(|meter| meter.integrated().blocks >= MIN_BLOCKS)
    }
    /// Use the normal measurement/target before any PCM is made audible. This
    /// consumes the existing decode preroll, never a second pass or cache.
    pub fn prime_av3a_startup(&mut self, origin: u64, ended: bool) -> Option<Command> {
        if !self.enabled || self.settled { return None; }
        let m = self.cached.clone().or_else(|| self.decoded_reference.clone()).or_else(|| self.meter.as_ref().map(LoudnessMeter::integrated))?;
        if !ended && m.blocks < MIN_BLOCKS { return None; }
        let lufs = m.integrated_lufs.filter(|v| v.is_finite())?;
        self.gain_db = master_gain(lufs, m.true_peak_dbtp);
        self.balanced_blocks = m.blocks;
        self.settled = true;
        Some(Command::SetProgramGain { gain: 10_f64.powf(self.gain_db / 20.0) as f32, at: Some(origin) })
    }
    pub fn route(&mut self, frame: &PendingFrame) -> Vec<Command> {
        let f = &frame.frame;
        let stereo = f.codec != "mpegh" && (stereo_master(f) || crate::alac_pcm::is_upmixed(f));
        let referenced = av3a_bed(f) || f.reference_loudness.is_some();
        if f.codec != "mpegh" && !referenced && !stereo {
            self.non_stereo_seen = true;
        }
        let eligible = !self.non_stereo_seen && (f.codec == "mpegh" || referenced || stereo);
        let mut commands = vec![];
        if eligible != self.eligible {
            self.eligible = eligible;
            commands.push(Command::SetProgramEnabled {
                enabled: self.enabled && eligible,
            });
        }
        if !eligible {
            return commands;
        }
        if let Some(cached) = &self.cached {
            if !self.settled {
                self.gain_db = master_gain(
                    cached.integrated_lufs.unwrap(),
                    if stereo || referenced { cached.true_peak_dbtp } else { None },
                );
                self.settled = true;
                commands.push(Command::SetProgramGain {
                    gain: 10_f64.powf(self.gain_db / 20.0) as f32,
                    at: Some(f.sample_pos),
                });
            }
        } else if let Some(m) = &frame.measurement {
            // Seven overlapping 400 ms windows cover one second of audible
            // material. Do not count a silent intro toward this estimate.
            let early = self.early_alac_balance && f.codec == "alac" && !self.settled;
            let required_blocks = if early { 7 } else { MIN_BLOCKS };
            if m.blocks >= required_blocks && (!self.settled || m.blocks >= self.balanced_blocks + 50) {
                if let Some(lufs) = m.integrated_lufs.filter(|v| v.is_finite()) {
                    self.settled = true;
                    self.balanced_blocks = m.blocks;
                    let gain = master_gain(lufs, if stereo || referenced { m.true_peak_dbtp } else { None });
                    let previous = self.gain_db;
                    self.gain_db = gain;
                    if early && (gain - previous).abs() >= 0.05 {
                        // Use the renderer's existing 50 ms linked ramp, not
                        // another multi-second startup staircase. The event is
                        // source-clocked so it cannot attenuate earlier audio.
                        commands.push(Command::SetProgramGain {
                            gain: 10_f64.powf(gain / 20.0) as f32,
                            at: Some(f.sample_pos),
                        });
                    } else if (gain - previous).abs() >= 0.05 {
                        let steps = ((gain - previous).abs() / 0.75).ceil() as u64;
                        let samples = (0.25_f64.min(4.0 / steps as f64) * f.sample_rate as f64)
                            .round() as u64;
                        for i in 1..=steps {
                            let db = if i == steps {
                                gain
                            } else {
                                previous + (gain - previous) * i as f64 / steps as f64
                            };
                            commands.push(Command::SetProgramGain {
                                gain: 10_f64.powf(db / 20.0) as f32,
                                at: Some(f.sample_pos + i * samples),
                            });
                        }
                    }
                }
            }
        }
        commands
    }
    pub fn finish(&mut self) {
        self.completed = true;
    }
    pub fn complete_measurement(&self) -> Option<Measurement> {
        // Object references are measured during this playback, not persisted as a stereo/bed cache.
        if self.decoded_reference.is_some() {return None;}
        let m = self.decoded_reference.clone().or_else(|| self.meter.as_ref().map(LoudnessMeter::integrated))?;
        (self.completed && self.eligible && m.blocks >= MIN_BLOCKS && m.integrated_lufs.is_some())
            .then_some(m)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn av3a_frame(at: u64) -> FrameData {
        let mut f = frame("av3a", at);
        f.labels = ["L","R","C","LFE","Lb","Rb","Ls","Rs","Tfl","Tfr","Tbl","Tbr"].iter().map(|s|s.to_string()).collect();
        f.raw_bed_labels = f.labels.clone(); f.channels = vec![vec![0.0;1024];12]; f
    }
    #[test]
    fn object_reference_is_eligible_without_treating_objects_as_speakers() {
        for codec in ["av3a","eac3","future-object-format"] {
            let mut b=VolumeBalance::default();b.enabled=true;
            let mut f=frame(codec,0);f.labels=vec!["Obj_42".into()];f.channels=vec![vec![0.;1024]];f.object_channels=vec![sda_core::ObjectChannelDecl{id:42,channel:0}];
            f.reference_loudness=Some(Measurement{integrated_lufs:Some(-10.),blocks:100,true_peak_dbtp:Some(-2.)});
            assert!(startup_program(&f));let pending=b.measure(f,None);
            assert!(b.startup_measurement_ready());assert!(b.prime_av3a_startup(0,false).is_some());
            let commands=b.route(&pending);assert!(b.eligible);assert_eq!(b.gain_db,-8.);
            assert!(commands.iter().any(|c|matches!(c,Command::SetProgramEnabled{enabled:true})));
            assert!(b.meter.is_none(),"shared decoded reference must not be measured a second time");
            b.set_cached(Measurement{integrated_lufs:Some(-2.),blocks:100,true_peak_dbtp:Some(-2.)});b.route(&pending);assert_eq!(b.gain_db,-8.);
            b.finish();assert!(b.complete_measurement().is_none());
            b.reset();assert!(!b.startup_measurement_ready());assert!(b.decoded_reference.is_none());
        }
    }
    #[test]
    fn av3a_balance_reuses_the_existing_stereo_policy_and_schedule() {
        let mut vivid=VolumeBalance {enabled:true,..Default::default()};
        let mut stereo=VolumeBalance {enabled:true,..Default::default()};
        for (at,blocks,lufs,peak) in [(0,0,None,3.0),(19200,1,Some(-5.0),3.0),(268800,56,Some(-5.0),3.0),
            (273600,57,Some(-5.0),3.0),(513600,107,Some(-1.0),3.0),(753600,157,Some(-28.0),-20.0),(993600,207,Some(-5.0),3.0)] {
            let m=Measurement {integrated_lufs:lufs,blocks,true_peak_dbtp:Some(peak)};
            let extract=|commands:Vec<Command>| commands.into_iter().filter_map(|c|if let Command::SetProgramGain{gain,at}=c {Some((gain,at))}else{None}).collect::<Vec<_>>();
            let actual=extract(vivid.route(&PendingFrame{frame:av3a_frame(at),measurement:Some(m.clone())}));
            let expected=extract(stereo.route(&PendingFrame{frame:frame("alac",at),measurement:Some(m)}));
            assert_eq!(actual,expected,"AV3A must not use a separate volume policy");
            assert_eq!(vivid.gain_db,stereo.gain_db);assert!(actual.iter().all(|(gain,_)|*gain<=1.0));
        }
    }
    #[test]
    fn av3a_balance_uses_twelve_bed_weights_with_existing_toggle_behavior() {
        let mut b=VolumeBalance::default();let pending=b.measure(av3a_frame(0),None);
        let commands=b.route(&pending);
        assert!(b.eligible);assert!(commands.iter().any(|c|matches!(c,Command::SetProgramEnabled{enabled:false})));
    }


    #[test]
    fn native_alac_startup_does_not_wait_six_seconds_or_schedule_seconds_ahead() {
        let mut b = VolumeBalance { enabled: true, ..Default::default() };
        b.enable_alac_startup();
        let early = PendingFrame {
            frame: frame("alac", 61440),
            measurement: Some(Measurement { integrated_lufs: Some(-10.0), blocks: 10, true_peak_dbtp: Some(-2.0) }),
        };
        let commands = b.route(&early);
        assert_eq!(b.gain_db, -8.0);
        assert!(commands.iter().any(|c| matches!(c, Command::SetProgramGain { gain, at: Some(61440) } if (*gain-10f32.powf(-8.0/20.0)).abs()<1e-6)));
        // A provisional estimate is never promoted to a full-track cache.
        assert!(b.complete_measurement().is_none());
    }

    #[test]
    fn native_alac_early_balance_respects_silence_cache_and_other_codecs() {
        let pending = |codec, blocks, lufs| PendingFrame {
            frame: frame(codec,48000),
            measurement: Some(Measurement { integrated_lufs:lufs, blocks, true_peak_dbtp:Some(-3.0) }),
        };
        let mut b=VolumeBalance::default(); b.enable_alac_startup();
        b.route(&pending("alac",0,None));
        assert!(!b.settled); assert_eq!(b.gain_db,0.0);
        b.route(&pending("alac",6,Some(-10.0)));
        assert!(!b.settled); assert_eq!(b.gain_db,0.0);
        b.route(&pending("alac",7,Some(-25.0)));
        assert!(b.settled); assert_eq!(b.gain_db,0.0); // Never boost quiet audio.
        b.reset(); b.enable_alac_startup();
        b.set_cached(Measurement { integrated_lufs:Some(-12.0), blocks:100, true_peak_dbtp:Some(-3.0) });
        b.route(&pending("alac",7,Some(-6.0)));
        assert_eq!(b.gain_db,-6.0); // Full measurement wins over provisional data.
        b.reset(); b.enable_alac_startup();
        b.route(&pending("mp3",7,Some(-10.0)));
        assert!(!b.settled); assert_eq!(b.gain_db,0.0);
        b.reset();
        b.route(&pending("alac",7,Some(-10.0)));
        assert!(!b.settled); // New sources cannot inherit the opt-in.
    }

    #[test]
    fn speaker_meter_excludes_lfe_and_retains_stereo_calibration() {
        let tone: Vec<f32> = (0..48000).map(|i| 0.5*(i as f32*std::f32::consts::TAU*1000.0/48000.0).sin()).collect();
        let mut lfe_only = vec![vec![0.0;48000];12]; lfe_only[3] = tone.clone();
        let mut meter = LoudnessMeter::speakers_7_1_4(); meter.push(&lfe_only);
        assert!(meter.integrated().integrated_lufs.is_none());
        let mut speakers = vec![vec![0.0;48000];12]; speakers[0] = tone.clone(); speakers[1] = tone.clone();
        let mut multichannel = LoudnessMeter::speakers_7_1_4(); multichannel.push(&speakers);
        let mut stereo = LoudnessMeter::new(48000); stereo.push(&[tone.clone(),tone]);
        assert_eq!(multichannel.integrated().integrated_lufs,stereo.integrated().integrated_lufs);
        assert_eq!(multichannel.integrated().true_peak_dbtp,stereo.integrated().true_peak_dbtp);
    }
    fn frame(codec: &'static str, at: u64) -> FrameData {
        FrameData { reference_loudness: None, compatibility_bed: None,
            codec,
            sample_rate: 48000,
            sample_pos: at,
            channels: vec![vec![0.0; 1024]; 2],
            labels: vec!["L".into(), "R".into()],
            raw_bed_labels: vec!["L".into(), "R".into()],
            object_channels: vec![],
            events: vec![],
            program_loudness: None,
            ramp_duration: 0,
        }
    }
    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!("balance-windows-fixtures.json")).unwrap()
    }
    #[test]
    fn windows_meter_vectors_match_at_all_interpolation_rates_and_chunk_sizes() {
        for wave in fixture()["waves"].as_array().unwrap() {
            let rate = wave["rate"].as_u64().unwrap() as u32;
            for chunk in [997, 4096] {
                let mut meter = LoudnessMeter::new(rate);
                let count = (rate as f64 * 6.4).round() as usize;
                for start in (0..count).step_by(chunk) {
                    let channels: Vec<Vec<f32>> = (0..2)
                        .map(|ch| {
                            (start..(start + chunk).min(count))
                                .map(|i| {
                                    let amp = if (i as f64) < rate as f64 * 1.1 {
                                        0.012
                                    } else {
                                        0.42
                                    };
                                    (amp * ((2.0 * std::f64::consts::PI * 997.0 * i as f64
                                        / rate as f64
                                        + ch as f64 * 0.3)
                                        .sin()
                                        + 0.13
                                            * (2.0 * std::f64::consts::PI * 17003.0 * i as f64
                                                / rate as f64)
                                                .sin())) as f32
                                })
                                .collect()
                        })
                        .collect();
                    meter.push(&channels);
                }
                let m = meter.integrated();
                assert_eq!(m.blocks, wave["blocks"].as_u64().unwrap() as usize);
                assert!(
                    (m.integrated_lufs.unwrap() - wave["integratedLufs"].as_f64().unwrap()).abs()
                        < 1e-6,
                    "rate={rate}"
                );
                assert!(
                    (m.true_peak_dbtp.unwrap() - wave["truePeakDbtp"].as_f64().unwrap()).abs()
                        < 1e-6,
                    "peak rate={rate}"
                );
            }
        }
        let mut silence = LoudnessMeter::new(48000);
        silence.push(&vec![vec![0.0; 48000]; 2]);
        assert_eq!(silence.integrated().integrated_lufs, None);
        assert_eq!(silence.integrated().blocks, 0);
        assert!(serde_json::to_string(&silence.integrated())
            .unwrap()
            .contains("null"));
    }
    #[test]
    fn windows_protective_gain_and_sample_clock_staircase_match() {
        for v in fixture()["gains"].as_array().unwrap() {
            assert_eq!(
                master_gain(v["lufs"].as_f64().unwrap(), v["peak"].as_f64()),
                v["gainDb"].as_f64().unwrap()
            );
        }
        assert_eq!(master_gain(f64::NAN, Some(-1.0)), 0.0);
        let mut balance = VolumeBalance {
            enabled: true,
            ..Default::default()
        };
        for step in fixture()["schedule"].as_array().unwrap() {
            let pending = PendingFrame {
                frame: frame("alac", step["at"].as_u64().unwrap()),
                measurement: Some(Measurement {
                    integrated_lufs: step["lufs"].as_f64(),
                    blocks: step["blocks"].as_u64().unwrap() as usize,
                    true_peak_dbtp: step["peak"].as_f64(),
                }),
            };
            let commands = balance.route(&pending);
            let gains: Vec<_> = commands
                .iter()
                .filter_map(|c| {
                    if let Command::SetProgramGain { gain, at } = c {
                        Some((*gain, *at))
                    } else {
                        None
                    }
                })
                .collect();
            let expected = step["calls"].as_array().unwrap();
            assert_eq!(gains.len(), expected.len());
            for ((gain, at), v) in gains.iter().zip(expected) {
                assert_eq!(*at, v["at"].as_u64());
                assert!(
                    (*gain as f64 - 10_f64.powf(v["gainDb"].as_f64().unwrap() / 20.0)).abs() < 1e-7
                );
            }
        }
    }
    #[test]
    fn cached_first_sample_sticky_eligibility_and_mpegh_never_uses_reference_peak_to_boost() {
        let mut b = VolumeBalance {
            enabled: true,
            ..Default::default()
        };
        b.set_cached(Measurement {
            integrated_lufs: Some(-23.0),
            blocks: 100,
            true_peak_dbtp: Some(2.0),
        });
        let p = PendingFrame {
            frame: frame("alac", 0),
            measurement: None,
        };
        let commands = b.route(&p);
        assert!(matches!(
            commands[0],
            Command::SetProgramEnabled { enabled: true }
        ));
        assert!(matches!(
            commands[1],
            Command::SetProgramGain { at: Some(0), .. }
        ));
        assert_eq!(b.gain_db, -3.0);
        assert!(b.route(&p).is_empty());
        let mut objects = frame("eac3", 1024);
        objects.labels[0] = "Obj_0".into();
        let commands = b.route(&PendingFrame {
            frame: objects,
            measurement: None,
        });
        assert!(matches!(
            commands[0],
            Command::SetProgramEnabled { enabled: false }
        ));
        assert!(b.route(&p).is_empty());
        assert!(!b.eligible);
        b.reset();
        assert!(b.enabled);
        assert!(!b.eligible);
        assert_eq!(b.gain_db, 0.0);
        b.set_cached(Measurement {
            integrated_lufs: Some(-23.0),
            blocks: 100,
            true_peak_dbtp: Some(2.0),
        });
        b.route(&PendingFrame {
            frame: frame("mpegh", 0),
            measurement: None,
        });
        assert_eq!(
            b.gain_db, 0.0,
            "reference peak is not SDA rendered true peak"
        );
        b.reset();
        b.set_cached(Measurement {
            integrated_lufs: Some(-10.0),
            blocks: 100,
            true_peak_dbtp: Some(2.0),
        });
        b.route(&PendingFrame {
            frame: frame("mpegh", 0),
            measurement: None,
        });
        assert_eq!(b.gain_db, -8.0);
    }
    #[test]
    fn only_complete_eligible_decodes_can_be_persisted_and_measurement_is_independent_of_toggle() {
        let mut b = VolumeBalance::default();
        let mut at = 0;
        for _ in 0..300 {
            let mut f = frame("alac", at);
            for ch in &mut f.channels {
                for (i, x) in ch.iter_mut().enumerate() {
                    *x = (0.5
                        * (2.0 * std::f64::consts::PI * 997.0 * (at + i as u64) as f64 / 48000.0)
                            .sin()) as f32;
                }
            }
            let p = b.measure(f, None);
            b.route(&p);
            at += 1024;
        }
        assert!(!b.enabled);
        assert!(b.gain_db < 0.0);
        assert!(
            b.complete_measurement().is_none(),
            "stopped/aborted intro must not cache"
        );
        b.finish();
        assert!(b.complete_measurement().is_some());
        b.reset();
        assert!(!b.enabled);
        assert!(b.complete_measurement().is_none());
    }
    #[test]
    fn source_classification_matches_windows_not_container_stereo_core() {
        let mut f = frame("alac", 0);
        assert!(stereo_master(&f));
        f.codec = "adm";
        assert!(!stereo_master(&f));
        f.codec = "eac3";
        f.labels[0] = "Obj_1".into();
        assert!(!stereo_master(&f));
        f.labels[0] = "C".into();
        assert!(!stereo_master(&f));
        f.labels = vec!["Left".into(), "Right".into()];
        assert!(stereo_master(&f));
        f.channels.push(vec![0.0; 1024]);
        assert!(!stereo_master(&f));
    }
}
