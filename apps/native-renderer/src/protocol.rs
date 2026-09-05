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
        Command::AddSource { id, at: _, bed_label } => {
            if state.sources.len() >= MAX_SOURCES && !state.sources.contains_key(&id) {
                write_event(&Event::Ack {
                    command: "addSource",
                    accepted: false,
                    detail: Some("source limit"),
                });
            } else {
                let sample_pos = state.sample_pos;
                let source_id = id.clone();
                let object_id = source_id
                    .strip_prefix("obj:")
                    .and_then(|value| value.parse::<u32>().ok());
                let is_object = object_id.is_some();
                let pending = if is_object {
                    state
                        .pending_object_events
                        .remove(&source_id)
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let bed_route = (!is_object)
                    .then(|| bed_route(bed_label.as_deref().unwrap_or(""), &state.vbap));
                let source = state.sources.entry(id).or_insert_with(|| Source {
                    gain: 1.0,
                    target_gain: 1.0,
                    ..Source::default()
                });
                source.kind = if is_object {
                    SourceKind::Object
                } else {
                    SourceKind::Bed
                };
                source.object_id = object_id;
                source.bed_label = (!is_object).then(|| bed_label.unwrap_or_else(|| "Bed_0".into()));
                source.activity_until = 0;
                if is_object {
                    for event in pending {
                        apply_object_event(source, sample_pos, event);
                    }
                } else if let Some(route) = bed_route {
                    let is_new_route = source.bus_gains == [0.0; vbap::MAX_BUS_COUNT]
                        && source.bus_targets == [0.0; vbap::MAX_BUS_COUNT]
                        && source.lfe_gain == 0.0
                        && source.lfe_target == 0.0;
                    // Bed declarations may arrive before their first PCM block.
                    // The ring itself already gates audibility at the codec `at`
                    // timestamp, so apply the semantic route now rather than
                    // retaining a precomputed vector whose indices may become
                    // stale if the user changes the virtual room first.
                    Engine::set_source_route(source, route, if is_new_route { 0 } else { 32 });
                }
                let routed = if is_object {
                    state.route_source_now(&source_id, 0)
                } else {
                    Ok(())
                };
                write_event(&Event::Ack {
                    command: "addSource",
                    accepted: routed.is_ok(),
                    detail: routed.err().as_deref(),
                });
            }
        }
        Command::ObjectEvents { events } => {
            for event in events {
                let id = format!("obj:{}", event.id);
                if state.sources.contains_key(&id) {
                    let applies_now = event.sample_pos <= state.sample_pos;
                    let elapsed = state.sample_pos.saturating_sub(event.sample_pos);
                    let ramp = if event.ramp_duration == 0 {
                        DEFAULT_OBJECT_RAMP
                    } else {
                        event.ramp_duration
                    };
                    {
                        let source = state.sources.get_mut(&id).expect("checked above");
                        apply_object_event(source, state.sample_pos, event);
                    }
                    if applies_now {
                        let _ = state.route_source_now(&id, ramp);
                        if elapsed > 0 {
                            let source = state.sources.get_mut(&id).expect("source still exists");
                            Engine::advance_source_envelopes(
                                source,
                                elapsed.min(u32::MAX as u64) as u32,
                            );
                        }
                    }
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
            // Throttle to one route rebuild per 20 ms and keep the newest pose;
            // the render loop interpolates between accepted poses per block, so
            // orientation motion stays smooth at the block cadence.
            const POSE_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_millis(20);
            let now = std::time::Instant::now();
            let throttled = state
                .last_pose_apply
                .is_some_and(|previous| now.duration_since(previous) < POSE_MIN_INTERVAL);
            let normalized = spatial::normalize_quaternion(orientation);
            let accepted = normalized.is_some();
            if let Some(pose) = normalized {
                state.pending_pose = Some(pose);
                if !throttled {
                    state.last_pose_apply = Some(now);
                    state.head_pose = Some(pose);
                    if state.pose_route_base.is_none() {
                        state.pose_route_base = Some(pose);
                    }
                }
            }
            if accepted && !throttled {
                let ids: Vec<String> = state
                    .sources
                    .keys()
                    .filter(|id| id.starts_with("obj:"))
                    .cloned()
                    .collect();
                for id in ids {
                    let _ = state.route_source_now(&id, convolution::DEFAULT_PARTITION as u32);
                }
            }
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
                        let prepared = state.rebuild_bus_renderer();
                        if prepared.is_ok() {
                            state.lfe_path.reset();
                        }
                        write_event(&Event::Ack {
                            command: "setHrtf",
                            accepted: prepared.is_ok(),
                            detail: prepared.err().as_deref(),
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
        Command::SetObjectHrtf { enabled } => {
            let result = state.set_direct_objects(enabled);
            write_event(&Event::Ack { command: "setObjectHrtf", accepted: result.is_ok(), detail: result.err().as_deref() });
        }
        Command::SetLayout { layout } => {
            match vbap::LayoutId::parse(&layout) {
                Some(layout) => match state.set_layout(layout) {
                    Ok(()) => {
                        // The bus graph owns partitioned-convolution history. Isolate
                        // this graph replacement with the existing FIFO reheat edge.
                        state.render_epoch = state.render_epoch.wrapping_add(1);
                        write_event(&Event::Ack {
                            command: "setLayout",
                            accepted: true,
                            detail: None,
                        });
                    }
                    Err(error) => write_event(&Event::Ack {
                        command: "setLayout",
                        accepted: false,
                        detail: Some(&error),
                    }),
                },
                None => write_event(&Event::Ack {
                    command: "setLayout",
                    accepted: false,
                    detail: Some("unsupported virtual speaker layout"),
                }),
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
                detail: (!accepted)
                    .then_some("configure a calibrated HRTF before enabling native output"),
            });
        }
        Command::StartAt { origin } => {
            let accepted = state.active_hrtf_set.is_some() && !state.sources.is_empty();
            if accepted {
                state.sample_pos = origin;
                state.block_offset = 0;
                state.output_active = true;
                state.paused = false;
                state.render_epoch = state.render_epoch.wrapping_add(1);
                if let Some(bus_renderer) = &mut state.bus_renderer {
                    bus_renderer.reset();
                }
                state.lfe_path.reset();
                for source in state.sources.values_mut() {
                    source.availability = 0.0;
                    source.availability_target = 0.0;
                    source.availability_ramp_remaining = 0;
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
            let ids: Vec<String> = state
                .sources
                .iter()
                .filter_map(|(id, source)| (source.kind == SourceKind::Object).then(|| id.clone()))
                .collect();
            for id in ids {
                let _ = state.route_source_now(&id, convolution::DEFAULT_PARTITION as u32);
            }
            write_event(&Event::Ack {
                command: "clearHeadPose",
                accepted: true,
                detail: None,
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
        Command::SetMuted { id, muted, at } => {
            let sample_pos = state.sample_pos;
            if let Some(source) = state.sources.get_mut(&id) {
                let at = at.unwrap_or(sample_pos);
                if at > sample_pos {
                    source.mute_events.insert(at, muted);
                } else {
                    source.muted = muted;
                }
                write_event(&Event::Ack {
                    command: "setMuted",
                    accepted: true,
                    detail: None,
                });
            } else {
                write_event(&Event::Ack {
                    command: "setMuted",
                    accepted: false,
                    detail: Some("unknown source"),
                });
            }
        }
        Command::SetLfeMuted { muted } => {
            state.lfe_muted = muted;
            // LFE is a direct path; clear its delayed/filter state so mute takes
            // effect at the control boundary instead of leaking a stale tail.
            if muted {
                state.lfe_path.reset();
            }
            write_event(&Event::Ack {
                command: "setLfeMuted",
                accepted: true,
                detail: None,
            });
        }
        Command::SetVolume { volume } => {
            if volume.is_finite() {
                state.set_output_volume(volume, !state.output_active);
                write_event(&Event::Ack {
                    command: "setVolume",
                    accepted: true,
                    detail: None,
                });
            } else {
                write_event(&Event::Ack {
                    command: "setVolume",
                    accepted: false,
                    detail: Some("invalid volume"),
                });
            }
        }
        Command::SetProgramEnabled { enabled } => {
            state.program_enabled = enabled;
            state.set_program_target(state.program_metadata_gain, !state.output_active);
            write_event(&Event::Ack {
                command: "setProgramEnabled",
                accepted: true,
                detail: None,
            });
        }
        Command::SetProgramGain { gain, at } => {
            if !gain.is_finite() {
                write_event(&Event::Ack {
                    command: "setProgramGain",
                    accepted: false,
                    detail: Some("invalid program gain"),
                });
            } else {
                let gain = gain.clamp(0.0, 1.0);
                let at = at.unwrap_or(state.sample_pos);
                if at > state.sample_pos {
                    state.program_events.insert(at, ProgramGainEvent { gain });
                } else {
                    state.program_metadata_gain = gain;
                    let immediate = !state.output_active;
                    state.set_program_target(gain, immediate);
                    if !immediate {
                        state.fast_forward_program_envelope(state.sample_pos.saturating_sub(at));
                    }
                }
                write_event(&Event::Ack {
                    command: "setProgramGain",
                    accepted: true,
                    detail: None,
                });
            }
        }
        Command::ClearHeadphoneCompensation => match headphone::HeadphoneCompensation::bypass() {
            Ok(compensation) => {
                state.headphone = compensation;
                state.render_epoch = state.render_epoch.wrapping_add(1);
                write_event(&Event::Ack {
                    command: "clearHeadphoneCompensation",
                    accepted: true,
                    detail: None,
                });
            }
            Err(error) => write_event(&Event::Ack {
                command: "clearHeadphoneCompensation",
                accepted: false,
                detail: Some(&error),
            }),
        },
        Command::SetBinauralEq {
            low,
            mid,
            high,
            low_cut,
        } => {
            if !low.is_finite() || !mid.is_finite() || !high.is_finite() {
                write_event(&Event::Ack {
                    command: "setBinauralEq",
                    accepted: false,
                    detail: Some("invalid binaural EQ"),
                });
            } else {
                match StereoEq::new(state.output_sample_rate, [low, mid, high], low_cut) {
                    Ok(eq) => {
                        state.binaural_eq = eq;
                        write_event(&Event::Ack {
                            command: "setBinauralEq",
                            accepted: true,
                            detail: None,
                        });
                    }
                    Err(error) => write_event(&Event::Ack {
                        command: "setBinauralEq",
                        accepted: false,
                        detail: Some(&error),
                    }),
                }
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
            state.reset_session(origin);
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

pub(super) fn apply_render_command(
    state: &mut Engine,
    command: render_command::RenderCommand,
    fifo: &stereo_fifo::StereoFifo,
    telemetry: &RuntimeTelemetry,
) -> bool {
    match command {
        render_command::RenderCommand::Command(command) => {
            handle_command(state, command, fifo, telemetry)
        }
        render_command::RenderCommand::Pcm { id, start, samples } => {
            ingest_pcm(state, &id, start, samples);
            true
        }
        render_command::RenderCommand::PcmBatch { start, entries } => {
            ingest_pcm_batch(state, start, entries);
            true
        }
        render_command::RenderCommand::HeadphoneFir {
            preamp,
            left,
            right,
        } => {
            match headphone::HeadphoneCompensation::new(&left, &right, preamp) {
                Ok(compensation) => {
                    state.headphone = compensation;
                    state.render_epoch = state.render_epoch.wrapping_add(1);
                    write_event(&Event::Ack {
                        command: "setHeadphoneFir",
                        accepted: true,
                        detail: None,
                    });
                }
                Err(error) => write_event(&Event::Ack {
                    command: "setHeadphoneFir",
                    accepted: false,
                    detail: Some(&error),
                }),
            }
            true
        }
    }
}

fn object_scalar_gain(gain_db: f32, position: [f32; 3]) -> f32 {
    if gain_db <= -128.0 {
        return 0.0;
    }
    let distance = position
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    let distance_gain = if distance > 1.0 {
        distance.recip()
    } else {
        1.0
    };
    10.0_f32.powf(gain_db / 20.0) * distance_gain
}

fn apply_object_event(source: &mut Source, sample_pos: u64, event: NativeObjectEvent) {
    let ramp = if event.ramp_duration == 0 {
        DEFAULT_OBJECT_RAMP
    } else {
        event.ramp_duration
    };
    if event.has_pos && event.pos.iter().all(|value| value.is_finite()) {
        let spatial = SpatialEvent {
            position: event.pos,
            spread: spatial::spread_from_size(event.size),
            ramp,
        };
        if event.sample_pos > sample_pos {
            source.spatial_events.insert(event.sample_pos, spatial);
        } else {
            source.position = spatial.position;
            source.spread = spatial.spread;
        }
    }
    if event.gain_db.is_finite() {
        let position = if event.has_pos && event.pos.iter().all(|value| value.is_finite()) {
            event.pos
        } else {
            source.position
        };
        let gain = object_scalar_gain(event.gain_db, position);
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
    if entries.is_empty()
        || entries.len() > MAX_SOURCES
        || samples == 0
        || entries.iter().any(|(_, pcm)| pcm.len() != samples)
    {
        write_event(&Event::BatchAck {
            start,
            samples: batch_samples,
            accepted: false,
            detail: Some("invalid batch"),
        });
        return;
    }
    // A replayed batch whose every sample is already behind the codec clock is an
    // ACK race, not an error: the render worker consumed the earlier copy, the
    // reply may have been lost, and the player retried. Accept it idempotently
    // (writing is a no-op for consumed slots) instead of rejecting it and making
    // the player drop the frame's objects audibly.
    let fully_stale = state.sample_pos.saturating_sub(start)
        >= entries.first().map_or(0, |(_, pcm)| pcm.len() as u64)
        && entries
            .iter()
            .all(|(id, pcm)| pcm.len() <= MAX_PENDING_SAMPLES && state.sources.contains_key(id));
    if fully_stale {
        write_event(&Event::BatchAck {
            start,
            samples: batch_samples,
            accepted: true,
            detail: Some("stale replay accepted idempotently"),
        });
        return;
    }
    let known_sources = entries.iter().all(|(id, pcm)| {
        pcm.len() <= MAX_PENDING_SAMPLES && state.sources.contains_key(id)
    });
    // A batch straddling the codec clock can still commit its future part. A
    // batch that is wholly unwritable ahead of the clock is a capacity error;
    // anything else unknown remains a source error.
    let valid = known_sources
        && entries.iter().all(|(id, pcm)| {
            state
                .sources
                .get(id)
                .is_some_and(|source| source.samples.can_write(state.sample_pos, start, pcm.len()))
                || state.sample_pos >= start + pcm.len() as u64
        });
    if !valid {
        write_event(&Event::BatchAck {
            start,
            samples: batch_samples,
            accepted: false,
            detail: Some("unknown source or source ring capacity"),
        });
        return;
    }
    for (id, pcm) in entries {
        let source = state
            .sources
            .get_mut(&id)
            .expect("batch pre-validation retains source");
        source.samples.write(state.sample_pos, start, &pcm);
    }
    write_event(&Event::BatchAck {
        start,
        samples: batch_samples,
        accepted: true,
        detail: None,
    });
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

fn read_f32_taps(input: &mut impl Read, count: usize) -> io::Result<Vec<f32>> {
    let mut raw = vec![0_u8; count * 4];
    input.read_exact(&mut raw)?;
    let taps: Vec<f32> = raw
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .collect();
    if taps.iter().all(|tap| tap.is_finite()) {
        Ok(taps)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "non-finite headphone FIR tap",
        ))
    }
}

pub(super) fn read_frames(
    input: &mut impl Read,
    commands: &Arc<render_command::RenderCommandQueue>,
) -> io::Result<()> {
    fn enqueue(
        queue: &render_command::RenderCommandQueue,
        command: render_command::RenderCommand,
    ) -> bool {
        queue.push(command).is_ok()
    }
    loop {
        let mut kind = [0_u8; 1];
        if read_exact_or_eof(input, &mut kind)? {
            return Ok(());
        }
        // A malformed frame used to bubble up and terminate the process, taking
        // playback with it. The writer can emit a torn frame under pipe
        // backpressure, so instead of exiting we report the frame and continue
        // with the next type byte: control commands self-heal on the next
        // submission, and PCM gaps fade through the availability ramp.
        let frame = read_frame(input, commands, enqueue, kind[0]);
        if let Err(error) = frame {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                return Ok(());
            }
            write_event(&Event::Error {
                detail: format!("frame dropped: {error}"),
            });
        }
    }
}

/// Reads one frame. Returns Ok(false) when the shutdown command was processed.
fn read_frame(
    input: &mut impl Read,
    commands: &Arc<render_command::RenderCommandQueue>,
    enqueue: fn(&render_command::RenderCommandQueue, render_command::RenderCommand) -> bool,
    kind: u8,
) -> io::Result<bool> {
    {
        match kind {
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
                if let Command::Hello { protocol } = command {
                    write_event(&Event::Ack {
                        command: "hello",
                        accepted: protocol == PROTOCOL,
                        detail: (protocol != PROTOCOL).then_some("protocol mismatch"),
                    });
                    return Ok(true);
                }
                let name = command_name(&command);
                let shutdown = matches!(command, Command::Shutdown);
                if !enqueue(commands, render_command::RenderCommand::Command(command)) {
                    write_event(&Event::Ack {
                        command: name,
                        accepted: false,
                        detail: Some("render command queue is full"),
                    });
                } else if shutdown {
                    return Ok(false);
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
                let mut raw_id = vec![0; id_length];
                input.read_exact(&mut raw_id)?;
                let id = String::from_utf8(raw_id)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid source id"))?;
                let mut raw = vec![0; count * 4];
                input.read_exact(&mut raw)?;
                let samples: Vec<f32> = raw
                    .chunks_exact(4)
                    .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                    .collect();
                if !enqueue(
                    commands,
                    render_command::RenderCommand::Pcm { id, start, samples },
                ) {
                    write_event(&Event::Ack {
                        command: "feed",
                        accepted: false,
                        detail: Some("render command queue is full"),
                    });
                }
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
                    let mut raw_id = vec![0; id_length];
                    input.read_exact(&mut raw_id)?;
                    let id = String::from_utf8(raw_id).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid source id")
                    })?;
                    let mut raw = vec![0; count * 4];
                    input.read_exact(&mut raw)?;
                    let samples: Vec<f32> = raw
                        .chunks_exact(4)
                        .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                        .collect();
                    entries.push((id, samples));
                }
                let samples = u32::try_from(entries.first().map_or(0, |(_, pcm)| pcm.len()))
                    .unwrap_or(u32::MAX);
                if !enqueue(
                    commands,
                    render_command::RenderCommand::PcmBatch { start, entries },
                ) {
                    write_event(&Event::BatchAck {
                        start,
                        samples,
                        accepted: false,
                        detail: Some("render command queue is full"),
                    });
                }
            }
            FRAME_HEADPHONE_FIR => {
                let mut preamp_bytes = [0_u8; 4];
                input.read_exact(&mut preamp_bytes)?;
                let preamp = f32::from_le_bytes(preamp_bytes);
                let left_count = read_u32(input)? as usize;
                let right_count = read_u32(input)? as usize;
                if !preamp.is_finite()
                    || preamp <= 0.0
                    || left_count < 2
                    || right_count < 2
                    || left_count > MAX_HEADPHONE_FIR_TAPS
                    || right_count > MAX_HEADPHONE_FIR_TAPS
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid headphone FIR header",
                    ));
                }
                let left = read_f32_taps(input, left_count)?;
                let right = read_f32_taps(input, right_count)?;
                if !enqueue(
                    commands,
                    render_command::RenderCommand::HeadphoneFir {
                        preamp,
                        left,
                        right,
                    },
                ) {
                    write_event(&Event::Ack {
                        command: "setHeadphoneFir",
                        accepted: false,
                        detail: Some("render command queue is full"),
                    });
                }
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unknown frame kind",
                ));
            }
        }
        Ok(true)
    }
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Hello { .. } => "hello",
        Command::Configure { .. } => "configure",
        Command::AddSource { .. } => "addSource",
        Command::RemoveSource { .. } => "removeSource",
        Command::Feed { .. } => "feed",
        Command::SetGain { .. } => "setGain",
        Command::SetMuted { .. } => "setMuted",
        Command::SetLfeMuted { .. } => "setLfeMuted",
        Command::SetVolume { .. } => "setVolume",
        Command::SetProgramEnabled { .. } => "setProgramEnabled",
        Command::SetProgramGain { .. } => "setProgramGain",
        Command::SetBinauralEq { .. } => "setBinauralEq",
        Command::ClearHeadphoneCompensation => "clearHeadphoneCompensation",
        Command::ObjectEvents { .. } => "objectEvents",
        Command::HeadPose { .. } => "headPose",
        Command::SetHrtf { .. } => "setHrtf",
        Command::SetLayout { .. } => "setLayout",
        Command::SetObjectHrtf { .. } => "setObjectHrtf",
        Command::SetOutputActive { .. } => "setOutputActive",
        Command::StartAt { .. } => "startAt",
        Command::ClearHeadPose => "clearHeadPose",
        Command::Pause { .. } => "pause",
        Command::Reset { .. } => "reset",
        Command::Health => "health",
        Command::Shutdown => "shutdown",
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
