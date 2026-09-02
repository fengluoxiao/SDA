use super::*;

fn handle_command(state: &mut Engine, command: Command) -> bool {
    match command {
        Command::Hello { protocol } => write_event(&Event::Ack { command: "hello", accepted: protocol == PROTOCOL, detail: (protocol != PROTOCOL).then_some("protocol mismatch") }),
        Command::Configure { sample_rate, channels } => {
            let matches_device = sample_rate == state.output_sample_rate && channels == state.output_channels;
            write_event(&Event::Ack { command: "configure", accepted: matches_device, detail: (!matches_device).then_some("device format is fixed for this sidecar instance") });
        }
        Command::AddSource { id } => {
            if state.sources.len() >= MAX_SOURCES && !state.sources.contains_key(&id) {
                write_event(&Event::Ack { command: "addSource", accepted: false, detail: Some("source limit") });
            } else {
                state.sources.entry(id).or_insert_with(|| Source { gain: 1.0, target_gain: 1.0, ..Source::default() });
                write_event(&Event::Ack { command: "addSource", accepted: true, detail: None });
            }
        }
        Command::RemoveSource { id, at } => {
            if let Some(source) = state.sources.get_mut(&id) { source.remove_at = Some(at); }
            write_event(&Event::Ack { command: "removeSource", accepted: true, detail: None });
        }
        Command::Feed { id, start, samples } => {
            ingest_pcm(state, &id, start, samples);
        }
        Command::SetGain { id, gain, ramp, at } => {
            let sample_pos = state.sample_pos;
            if let Some(source) = state.sources.get_mut(&id) {
                let at = at.unwrap_or(sample_pos);
                if at > sample_pos {
                    source.gain_events.insert(at, GainEvent { gain, ramp: ramp.max(1) });
                } else {
                    source.target_gain = gain;
                    source.ramp_remaining = ramp.max(1);
                    source.ramp_step = (gain - source.gain) / source.ramp_remaining as f32;
                }
                write_event(&Event::Ack { command: "setGain", accepted: true, detail: None });
            } else { write_event(&Event::Ack { command: "setGain", accepted: false, detail: Some("unknown source") }); }
        }
        Command::Pause { paused } => { state.paused = paused; write_event(&Event::Ack { command: "pause", accepted: true, detail: None }); }
        Command::Reset { origin } => { state.sample_pos = origin; for source in state.sources.values_mut() { source.samples.clear(); source.gain_events.clear(); } write_event(&Event::Ack { command: "reset", accepted: true, detail: None }); }
        Command::Health => write_event(&Event::Health(state.health())),
        Command::Shutdown => { write_event(&Event::Ack { command: "shutdown", accepted: true, detail: None }); return false; }
    }
    true
}

fn ingest_pcm(state: &mut Engine, id: &str, start: u64, samples: Vec<f32>) {
    let Some(source) = state.sources.get_mut(id) else { write_event(&Event::Ack { command: "feed", accepted: false, detail: Some("unknown source") }); return; };
    if source.samples.len().saturating_add(samples.len()) > MAX_PENDING_SAMPLES {
        write_event(&Event::Ack { command: "feed", accepted: false, detail: Some("source ring capacity") });
        return;
    }
    for (offset, sample) in samples.into_iter().enumerate() { source.samples.insert(start + offset as u64, sample); }
    write_event(&Event::Ack { command: "feed", accepted: true, detail: None });
}

fn read_exact_or_eof(input: &mut impl Read, bytes: &mut [u8]) -> io::Result<bool> {
    let mut offset = 0;
    while offset < bytes.len() {
        let read = input.read(&mut bytes[offset..])?;
        if read == 0 { return Ok(offset == 0); }
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

pub(super) fn read_frames(input: &mut impl Read, engine: &Arc<Mutex<Engine>>) -> io::Result<()> {
    loop {
        let mut kind = [0_u8; 1];
        if read_exact_or_eof(input, &mut kind)? { return Ok(()); }
        match kind[0] {
            FRAME_JSON => {
                let length = read_u32(input)? as usize;
                if length > NATIVE_RENDERER_MAX_JSON_BYTES { return Err(io::Error::new(io::ErrorKind::InvalidData, "JSON frame exceeds limit")); }
                let mut bytes = vec![0; length];
                input.read_exact(&mut bytes)?;
                let command = serde_json::from_slice::<Command>(&bytes)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                let mut state = engine.lock().map_err(|_| io::Error::other("engine lock poisoned"))?;
                if !handle_command(&mut state, command) { return Ok(()); }
            }
            FRAME_PCM => {
                let id_length = read_u16(input)? as usize;
                let start = read_u64(input)?;
                let count = read_u32(input)? as usize;
                if id_length == 0 || id_length > 128 || count > MAX_PENDING_SAMPLES { return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid PCM frame header")); }
                let mut id = vec![0; id_length];
                input.read_exact(&mut id)?;
                let id = String::from_utf8(id).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid source id"))?;
                let mut raw = vec![0; count * 4];
                input.read_exact(&mut raw)?;
                let samples = raw.chunks_exact(4).map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])).collect();
                let mut state = engine.lock().map_err(|_| io::Error::other("engine lock poisoned"))?;
                ingest_pcm(&mut state, &id, start, samples);
            }
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "unknown frame kind")),
        }
    }
}
