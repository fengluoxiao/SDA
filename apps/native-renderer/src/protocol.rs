use super::*;

fn handle_command(
    state: &mut Engine,
    command: Command,
    fifo: &stereo_fifo::StereoFifo,
    telemetry: &RuntimeTelemetry,
) -> bool {
    match command {
        Command::Hello { protocol } => write_event(&Event::Ack {
            command: "hello",
            accepted: protocol == PROTOCOL,
            detail: (protocol != PROTOCOL).then_some("protocol mismatch"),
        }),
        Command::Configure {
            sample_rate,
            channels,
        } => {
            let matches_device =
                sample_rate == state.output_sample_rate && channels == state.output_channels;
            write_event(&Event::Ack {
                command: "configure",
                accepted: matches_device,
                detail: (!matches_device)
                    .then_some("device format is fixed for this sidecar instance"),
            });
        }
        Command::AddSource { id } => {
            if state.sources.len() >= MAX_SOURCES && !state.sources.contains_key(&id) {
                write_event(&Event::Ack {
                    command: "addSource",
                    accepted: false,
                    detail: Some("source limit"),
                });
            } else {
                let pending = state.pending_object_events.remove(&id).unwrap_or_default();
                let sample_pos = state.sample_pos;
                let source_id = id.clone();
                let source = state.sources.entry(id).or_insert_with(|| Source {
                    gain: 1.0,
                    target_gain: 1.0,
                    ..Source::default()
                });
                for event in pending {
                    apply_object_event(source, sample_pos, event);
                }
                let refreshed = if source_id.starts_with("obj:") && state.active_hrtf_set.is_some()
                {
                    state.refresh_source_hrtf(&source_id)
                } else {
                    Ok(())
                };
                write_event(&Event::Ack {
                    command: "addSource",
                    accepted: refreshed.is_ok(),
                    detail: refreshed.err().as_deref(),
                });
            }
        }
        Command::ObjectEvents { events } => {
            for event in events {
                let id = format!("obj:{}", event.id);
                if let Some(source) = state.sources.get_mut(&id) {
                    // Future events are applied on the audio timeline. Do not build
                    // filters while holding the engine lock on every decoded frame.
                    apply_object_event(source, state.sample_pos, event);
                } else {
                    // Decoders can emit OAMD before the matching PCM declaration.
                    // Preserve the codec timestamp and apply it when addSource arrives.
                    state
                        .pending_object_events
                        .entry(id)
                        .or_default()
                        .push(event);
                }
            }
            write_event(&Event::Ack {
                command: "objectEvents",
                accepted: true,
                detail: None,
            });
        }
        Command::RemoveSource { id, at } => {
            if let Some(source) = state.sources.get_mut(&id) {
                source.remove_at = Some(at);
            }
            write_event(&Event::Ack {
                command: "removeSource",
                accepted: true,
                detail: None,
            });
        }
        Command::Feed { id, start, samples } => {
            ingest_pcm(state, &id, start, samples);
        }
        Command::HeadPose { orientation } => {
            // Pose delivery can run at 60–120 Hz. Rebuilding every source filter
            // here holds the shared render lock long enough to cause WASAPI xruns.
            // Keep the newest pose; filter transitions are prepared separately.
            state.head_pose = spatial::normalize_quaternion(orientation);
            let accepted = state.head_pose.is_some();
            write_event(&Event::Ack {
                command: "headPose",
                accepted,
                detail: (!accepted).then_some("invalid quaternion"),
            });
        }
        Command::SetHrtf { set, wet_weight } => {
            let safe = matches!(
                set.as_str(),
                "hrtf"
                    | "hrtf-dense"
                    | "hrtf-d2"
                    | "hrtf-h3"
                    | "hrtf-h4"
                    | "hrtf-h5"
                    | "hrtf-h6"
                    | "hrtf-h7"
                    | "hrtf-h8"
                    | "hrtf-h9"
                    | "hrtf-h10"
                    | "hrtf-h11"
                    | "hrtf-h12"
                    | "hrtf-h13"
                    | "hrtf-h14"
                    | "hrtf-h15"
                    | "hrtf-h16"
                    | "hrtf-h17"
                    | "hrtf-h18"
                    | "hrtf-h19"
                    | "hrtf-h20"
            );
            if !safe || !wet_weight.is_finite() {
                write_event(&Event::Ack {
                    command: "setHrtf",
                    accepted: false,
                    detail: Some("invalid HRTF set or wet weight"),
                });
            } else {
                let manifest = Engine::hrtf_root().join(&set).join("hrtf-set.json");
                match hrtf::NativeHrtfSet::load_calibrated(&manifest) {
                    Ok(loaded) if loaded.sample_rate == 48_000 => {
                        state.active_hrtf_set = Some(loaded);
                        state.hrtf_wet_weight = wet_weight.clamp(0.0, 1.0);
                        write_event(&Event::Ack {
                            command: "setHrtf",
                            accepted: true,
                            detail: None,
                        });
                    }
                    Ok(_) => write_event(&Event::Ack {
                        command: "setHrtf",
                        accepted: false,
                        detail: Some("native renderer requires 48k HRTF assets"),
                    }),
                    Err(error) => write_event(&Event::Ack {
                        command: "setHrtf",
                        accepted: false,
                        detail: Some(&error),
                    }),
                }
            }
        }
        Command::SetOutputActive { active } => {
            let accepted = !active || state.active_hrtf_set.is_some();
            if accepted && state.output_active != active {
                state.output_active = active;
                state.render_epoch = state.render_epoch.wrapping_add(1);
            }
            write_event(&Event::Ack {
                command: "setOutputActive",
                accepted,
                detail: (!accepted).then_some("configure a calibrated HRTF before enabling native output"),
            });
        }
        Command::StartAt { origin } => {
            let accepted = state.active_hrtf_set.is_some()
                && !state.sources.is_empty();
            if accepted {
                state.sample_pos = origin;
                state.block_offset = 0;
                state.output_active = true;
                state.paused = false;
                state.render_epoch = state.render_epoch.wrapping_add(1);
                for source in state.sources.values_mut() {
                    if let Some(convolver) = &mut source.convolver { convolver.reset(); }
                    source.input_block.fill(0.0);
                    source.output_left.fill(0.0);
                    source.output_right.fill(0.0);
                }
            }
            write_event(&Event::Ack {
                command: "startAt",
                accepted,
                detail: (!accepted).then_some("configure sources, calibrated HRTF, and prebuffer before starting native output"),
            });
        }
        Command::ClearHeadPose => {
            state.head_pose = None;
            let refreshed = state.refresh_all_hrtf();
            write_event(&Event::Ack {
                command: "clearHeadPose",
                accepted: refreshed.is_ok(),
                detail: refreshed.err().as_deref(),
            });
        }
        Command::SetGain { id, gain, ramp, at } => {
            let sample_pos = state.sample_pos;
            if let Some(source) = state.sources.get_mut(&id) {
                let at = at.unwrap_or(sample_pos);
                if at > sample_pos {
                    source.gain_events.insert(
                        at,
                        GainEvent {
                            gain,
                            ramp: ramp.max(1),
                        },
                    );
                } else {
                    source.target_gain = gain;
                    source.ramp_remaining = ramp.max(1);
                    source.ramp_step = (gain - source.gain) / source.ramp_remaining as f32;
                }
                write_event(&Event::Ack {
                    command: "setGain",
                    accepted: true,
                    detail: None,
                });
            } else {
                write_event(&Event::Ack {
                    command: "setGain",
                    accepted: false,
                    detail: Some("unknown source"),
                });
            }
        }
        Command::Pause { paused } => {
            if state.paused != paused {
                state.paused = paused;
                state.render_epoch = state.render_epoch.wrapping_add(1);
            }
            write_event(&Event::Ack {
                command: "pause",
                accepted: true,
                detail: None,
            });
        }
        Command::Reset { origin } => {
            state.sample_pos = origin;
            state.block_offset = 0;
            state.render_epoch = state.render_epoch.wrapping_add(1);
            state.pending_object_events.clear();
            for source in state.sources.values_mut() {
                source.samples.clear();
                source.gain_events.clear();
                source.spatial_events.clear();
                source.remove_at = None;
                source.gain = 1.0;
                source.target_gain = 1.0;
                source.ramp_remaining = 0;
                source.ramp_step = 0.0;
                source.input_block.fill(0.0);
                source.output_left.fill(0.0);
                source.output_right.fill(0.0);
                if let Some(convolver) = &mut source.convolver { convolver.reset(); }
            }
            write_event(&Event::Ack {
                command: "reset",
                accepted: true,
                detail: None,
            });
        }
        Command::Health => write_event(&Event::Health(state.health(fifo, telemetry))),
        Command::Shutdown => {
            write_event(&Event::Ack {
                command: "shutdown",
                accepted: true,
                detail: None,
            });
            return false;
        }
    }
    true
}

fn apply_object_event(source: &mut Source, sample_pos: u64, event: NativeObjectEvent) {
    if event.has_pos && event.pos.iter().all(|value| value.is_finite()) {
        let spatial = SpatialEvent { position: event.pos, spread: spatial::spread_from_size(event.size) };
        if event.sample_pos > sample_pos { source.spatial_events.insert(event.sample_pos, spatial); }
        else { source.position = spatial.position; source.spread = spatial.spread; }
    }
    if event.gain_db.is_finite() {
        let gain = 10.0_f32.powf(event.gain_db / 20.0);
        let ramp = event.ramp_duration.max(1);
        if event.sample_pos > sample_pos {
            source
                .gain_events
                .insert(event.sample_pos, GainEvent { gain, ramp });
        } else {
            source.target_gain = gain;
            source.ramp_remaining = ramp;
            source.ramp_step = (gain - source.gain) / ramp as f32;
        }
    }
}

fn ingest_pcm_batch(state: &mut Engine, start: u64, entries: Vec<(String, Vec<f32>)>) {
    let samples = entries.first().map_or(0, |(_, pcm)| pcm.len());
    let batch_samples = u32::try_from(samples).unwrap_or(u32::MAX);
    if entries.is_empty() || entries.len() > MAX_SOURCES || samples == 0 || entries.iter().any(|(_, pcm)| pcm.len() != samples) {
        write_event(&Event::BatchAck { start, samples: batch_samples, accepted: false, detail: Some("invalid batch") });
        return;
    }
    let valid = entries.iter().all(|(id, pcm)| {
        pcm.len() <= MAX_PENDING_SAMPLES
            && state.sources.get(id).is_some_and(|source| source.samples.can_write(state.sample_pos, start, pcm.len()))
    });
    if !valid {
        write_event(&Event::BatchAck { start, samples: batch_samples, accepted: false, detail: Some("unknown source or source ring capacity") });
        return;
    }
    for (id, pcm) in entries {
        let source = state.sources.get_mut(&id).expect("batch pre-validation retains source");
        source.samples.write(state.sample_pos, start, &pcm);
    }
    write_event(&Event::BatchAck { start, samples: batch_samples, accepted: true, detail: None });
}

fn ingest_pcm(state: &mut Engine, id: &str, start: u64, samples: Vec<f32>) {
    let Some(source) = state.sources.get_mut(id) else {
        write_event(&Event::Ack {
            command: "feed",
            accepted: false,
            detail: Some("unknown source"),
        });
        return;
    };
    if !source
        .samples
        .can_write(state.sample_pos, start, samples.len())
    {
        write_event(&Event::Ack {
            command: "feed",
            accepted: false,
            detail: Some("source ring capacity"),
        });
        return;
    }
    source.samples.write(state.sample_pos, start, &samples);
    write_event(&Event::Ack {
        command: "feed",
        accepted: true,
        detail: None,
    });
}

fn read_exact_or_eof(input: &mut impl Read, bytes: &mut [u8]) -> io::Result<bool> {
    let mut offset = 0;
    while offset < bytes.len() {
        let read = input.read(&mut bytes[offset..])?;
        if read == 0 {
            return Ok(offset == 0);
        }
        offset += read;
    }
    Ok(false)
}

fn read_u16(input: &mut impl Read) -> io::Result<u16> {
    let mut bytes = [0; 2];
    input.read_exact(&mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(input: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(input: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0; 8];
    input.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

pub(super) fn read_frames(
    input: &mut impl Read,
    engine: &Arc<Mutex<Engine>>,
    fifo: &Arc<stereo_fifo::StereoFifo>,
    telemetry: &Arc<RuntimeTelemetry>,
) -> io::Result<()> {
    loop {
        let mut kind = [0_u8; 1];
        if read_exact_or_eof(input, &mut kind)? {
            return Ok(());
        }
        match kind[0] {
            FRAME_JSON => {
                let length = read_u32(input)? as usize;
                if length > NATIVE_RENDERER_MAX_JSON_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "JSON frame exceeds limit",
                    ));
                }
                let mut bytes = vec![0; length];
                input.read_exact(&mut bytes)?;
                let command = serde_json::from_slice::<Command>(&bytes)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                let mut state = engine
                    .lock()
                    .map_err(|_| io::Error::other("engine lock poisoned"))?;
                if !handle_command(&mut state, command, fifo, telemetry) {
                    return Ok(());
                }
            }
            FRAME_PCM => {
                let id_length = read_u16(input)? as usize;
                let start = read_u64(input)?;
                let count = read_u32(input)? as usize;
                if id_length == 0 || id_length > 128 || count > MAX_PENDING_SAMPLES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid PCM frame header",
                    ));
                }
                let mut id = vec![0; id_length];
                input.read_exact(&mut id)?;
                let id = String::from_utf8(id)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid source id"))?;
                let mut raw = vec![0; count * 4];
                input.read_exact(&mut raw)?;
                let samples = raw
                    .chunks_exact(4)
                    .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                    .collect();
                let mut state = engine
                    .lock()
                    .map_err(|_| io::Error::other("engine lock poisoned"))?;
                ingest_pcm(&mut state, &id, start, samples);
            }
            FRAME_PCM_BATCH => {
                let start = read_u64(input)?;
                let entry_count = read_u16(input)? as usize;
                if entry_count == 0 || entry_count > MAX_SOURCES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid PCM batch entry count",
                    ));
                }
                let mut entries = Vec::with_capacity(entry_count);
                for _ in 0..entry_count {
                    let id_length = read_u16(input)? as usize;
                    let count = read_u32(input)? as usize;
                    if id_length == 0
                        || id_length > 128
                        || count == 0
                        || count > MAX_PENDING_SAMPLES
                    {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid PCM batch entry header",
                        ));
                    }
                    let mut id = vec![0; id_length];
                    input.read_exact(&mut id)?;
                    let id = String::from_utf8(id).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid source id")
                    })?;
                    let mut raw = vec![0; count * 4];
                    input.read_exact(&mut raw)?;
                    let samples = raw
                        .chunks_exact(4)
                        .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                        .collect();
                    entries.push((id, samples));
                }
                let mut state = engine
                    .lock()
                    .map_err(|_| io::Error::other("engine lock poisoned"))?;
                ingest_pcm_batch(&mut state, start, entries);
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown frame kind",
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_pcm_batch_does_not_partially_write_any_source() {
        let mut engine = Engine::new(48_000, 2);
        engine.sources.insert("obj:1".into(), Source::default());

        ingest_pcm_batch(
            &mut engine,
            100,
            vec![("obj:1".into(), vec![0.25]), ("unknown".into(), vec![0.5])],
        );

        assert_eq!(
            engine.sources.get_mut("obj:1").unwrap().samples.take(100),
            None
        );
    }
}
