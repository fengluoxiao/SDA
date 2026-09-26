//! Audio output lifecycle, isolated from the render worker and its state.
//! Windows: WASAPI exclusive/shared via the `windows` crate.
//! macOS/Linux: CPAL (CoreAudio/ALSA) callback-driven stream.
use super::*;
use std::sync::{OnceLock, mpsc};
#[cfg(windows)]
#[path = "asio_output.rs"]
mod asio_output;
#[cfg(windows)]
#[path = "directsound_output.rs"]
mod directsound_output;

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub device_id: Option<String>,
    #[serde(default)]
    pub exclusive: bool,
    #[serde(default)]
    pub remote_compatible: bool,
}
impl Settings {
    fn normalized(mut self) -> Self {
        if self.remote_compatible {
            self.exclusive = false;
            self.device_id = None;
        } else if self
            .device_id
            .as_deref()
            .is_some_and(|id| id.starts_with("asio:") || id.starts_with("dsound:"))
        {
            self.exclusive = false;
        }
        self
    }
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    id: String,
    name: String,
    available: bool,
    is_default: bool,
    sample_rate: Option<u32>,
    channels: Option<u16>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub requested: Settings,
    actual_id: Option<String>,
    actual_name: Option<String>,
    mode: Option<String>,
    sample_rate: Option<u32>,
    channels: Option<u16>,
    buffer_ms: Option<f64>,
    sample_format: Option<String>,
    state: String,
    detail: String,
}
enum Request {
    ControlPanel,
    List,
    Set(Settings),
    Devices(Vec<Endpoint>),
    Stop,
    Remote(Option<(String, String)>),
}

static CONTROL: OnceLock<mpsc::Sender<Request>> = OnceLock::new();
pub fn request(settings: Option<Settings>) {
    let settings = settings.map(Settings::normalized);
    let name = if settings.is_some() {
        "setOutputDevice"
    } else {
        "listOutputDevices"
    };
    let accepted = CONTROL.get().is_some_and(|tx| {
        tx.send(settings.map_or(Request::List, Request::Set))
            .is_ok()
    });
    if !accepted {
        write_event(&Event::Ack {
            command: name,
            accepted: false,
            detail: Some("output manager unavailable"),
        });
    }
}
pub fn remote_request(address: Option<String>, token: Option<String>) {
    let accepted = CONTROL
        .get()
        .is_some_and(|tx| tx.send(Request::Remote(address.zip(token))).is_ok());
    if !accepted {
        write_event(&Event::Ack {
            command: "setRemoteOutput",
            accepted: false,
            detail: Some("output manager unavailable"),
        });
    }
}
pub fn control_panel() {
    if !CONTROL
        .get()
        .is_some_and(|tx| tx.send(Request::ControlPanel).is_ok())
    {
        write_event(&Event::Ack {
            command: "openAsioControlPanel",
            accepted: false,
            detail: Some("output manager unavailable"),
        });
    }
}
pub fn stop() {
    if let Some(tx) = CONTROL.get() {
        let _ = tx.send(Request::Stop);
    }
}

// ── Windows WASAPI implementation ───────────────────────────────────────────
#[cfg(windows)]
mod platform {
    use super::*;
    struct ComScope;
    impl ComScope {
        fn new() -> Result<Self, String> {
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                    .ok()
                    .map_err(|e| e.to_string())?;
            }
            Ok(Self)
        }
    }
    impl Drop for ComScope {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    struct TimerScope;
    impl Drop for TimerScope {
        fn drop(&mut self) {
            unsafe {
                windows::Win32::Media::timeEndPeriod(1);
            }
        }
    }
    use windows::{
        Win32::{
            Devices::Properties::DEVPKEY_Device_FriendlyName, Media::Audio::*, System::Com::*,
        },
        core::PCWSTR,
    };

    fn endpoint_id(device: &IMMDevice) -> Result<String, String> {
        unsafe {
            let ptr = device.GetId().map_err(|e| e.to_string())?;
            let value = ptr.to_string().map_err(|e| e.to_string());
            CoTaskMemFree(Some(ptr.0.cast()));
            value
        }
    }
    fn endpoint_name(device: &IMMDevice) -> String {
        unsafe {
            device
                .OpenPropertyStore(STGM_READ)
                .and_then(|store| {
                    store.GetValue(&DEVPKEY_Device_FriendlyName as *const _ as *const _)
                })
                .map(|value| value.to_string())
                .unwrap_or_else(|_| "Audio output".into())
        }
    }
    fn enumerator() -> Result<IMMDeviceEnumerator, String> {
        unsafe {
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())
        }
    }
    fn default_id(e: &IMMDeviceEnumerator) -> Option<String> {
        unsafe {
            e.GetDefaultAudioEndpoint(eRender, eConsole)
                .ok()
                .and_then(|d| endpoint_id(&d).ok())
        }
    }
    fn available(device: &IMMDevice) -> bool {
        unsafe {
            let mut state = 0;
            device.GetState(&mut state).0 == 0 && state == DEVICE_STATE_ACTIVE.0
        }
    }
    fn endpoint_format(bytes: &[u8]) -> Option<(u32, u16)> {
        if bytes.len() < 8 {
            return None;
        }
        let channels = u16::from_le_bytes(bytes[2..4].try_into().ok()?);
        let rate = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
        (channels > 0 && channels <= 32 && (8000..=768000).contains(&rate))
            .then_some((rate, channels))
    }
    fn list(e: &IMMDeviceEnumerator) -> Result<Vec<Endpoint>, String> {
        unsafe {
            let default = default_id(e);
            let collection = e
                .EnumAudioEndpoints(eRender, DEVICE_STATE(DEVICE_STATEMASK_ALL))
                .map_err(|e| e.to_string())?;
            let mut result = Vec::new();
            for i in 0..collection.GetCount().map_err(|e| e.to_string())? {
                let device = collection.Item(i).map_err(|e| e.to_string())?;
                let id = endpoint_id(&device)?;
                let available = available(&device);
                // Read the endpoint's format property without activating a fresh audio
                // client every second. Device discovery must not touch the live stream.
                let format = device
                    .OpenPropertyStore(STGM_READ)
                    .ok()
                    .and_then(|store| store.GetValue(&PKEY_AudioEngine_DeviceFormat).ok())
                    .and_then(|value| {
                        let raw = value.as_raw().Anonymous.Anonymous;
                        if raw.vt != windows::Win32::System::Variant::VT_BLOB.0 {
                            return None;
                        }
                        let blob = raw.Anonymous.blob;
                        if blob.pBlobData.is_null() || blob.cbSize < 8 {
                            return None;
                        }
                        endpoint_format(std::slice::from_raw_parts(
                            blob.pBlobData,
                            blob.cbSize as usize,
                        ))
                    });
                result.push(Endpoint {
                    is_default: default.as_ref() == Some(&id),
                    id,
                    name: endpoint_name(&device),
                    available,
                    sample_rate: format.map(|v| v.0),
                    channels: format.map(|v| v.1),
                });
            }
            result.sort_by(|a, b| {
                b.available
                    .cmp(&a.available)
                    .then(a.name.cmp(&b.name))
                    .then(a.id.cmp(&b.id))
            });
            result.extend(asio_output::endpoints());
            result.extend(directsound_output::endpoints());
            Ok(result)
        }
    }
    struct WasapiOutput {
        client: IAudioClient,
        render: IAudioRenderClient,
        id: String,
        name: String,
        settings: Settings,
        rate: u32,
        channels: usize,
        bits: u16,
        float: bool,
        size: u32,
        converter: device_output::DeviceOutput,
        monitor: output_monitor::OutputMonitor,
        fade: f32,
        lossless: bool,
        event: Option<OutputEvent>,
    }
    struct OutputEvent(windows::Win32::Foundation::HANDLE);
    impl Drop for OutputEvent {
        fn drop(&mut self) {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }
    fn initialize_client(
        device: &IMMDevice,
        format: &WAVEFORMATEXTENSIBLE,
        exclusive: bool,
    ) -> Result<IAudioClient, String> {
        unsafe {
            let mode = if exclusive {
                AUDCLNT_SHAREMODE_EXCLUSIVE
            } else {
                AUDCLNT_SHAREMODE_SHARED
            };
            let mut client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| e.to_string())?;
            if exclusive {
                client
                    .IsFormatSupported(mode, &format.Format, None)
                    .ok()
                    .map_err(|e| e.to_string())?;
            }
            let mut period = 0;
            client
                .GetDevicePeriod(None, Some(&mut period))
                .map_err(|e| e.to_string())?;
            let duration = if exclusive {
                period.max(100000)
            } else {
                200000
            };
            let result = client.Initialize(
                mode,
                if exclusive {
                    0
                } else {
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                },
                duration,
                if exclusive { duration } else { 0 },
                &format.Format,
                None,
            );
            if exclusive
                && result
                    .as_ref()
                    .is_err_and(|e| e.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED)
            {
                let frames = client.GetBufferSize().map_err(|e| e.to_string())?;
                let aligned = (10_000_000.0 * frames as f64 / format.Format.nSamplesPerSec as f64)
                    .round() as i64;
                drop(client);
                client = device
                    .Activate(CLSCTX_ALL, None)
                    .map_err(|e| e.to_string())?;
                client
                    .Initialize(mode, 0, aligned, aligned, &format.Format, None)
                    .map_err(|e| e.to_string())?;
            } else {
                result.map_err(|e| e.to_string())?;
            }
            Ok(client)
        }
    }
    impl Drop for WasapiOutput {
        fn drop(&mut self) {
            unsafe {
                let _ = self.client.Stop();
            }
        }
    }
    impl WasapiOutput {
        fn open(e: &IMMDeviceEnumerator, settings: &Settings) -> Result<Self, String> {
            unsafe {
                let device = if let Some(id) = &settings.device_id {
                    if id.len() > 1024 || id.contains('\0') {
                        return Err("invalid output endpoint ID".into());
                    }
                    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
                    e.GetDevice(PCWSTR(wide.as_ptr()))
                } else {
                    e.GetDefaultAudioEndpoint(eRender, eConsole)
                }
                .map_err(|e| e.to_string())?;
                if !available(&device) {
                    return Err("selected output is disconnected or disabled".into());
                }
                let mut client: IAudioClient = device
                    .Activate(CLSCTX_ALL, None)
                    .map_err(|e| e.to_string())?;
                let mix = client.GetMixFormat().map_err(|e| e.to_string())?;
                let mut format = WAVEFORMATEXTENSIBLE::default();
                format.Format = *mix;
                if (*mix).wFormatTag == 65534 && (*mix).cbSize >= 22 {
                    format = std::ptr::read_unaligned(mix.cast());
                }
                CoTaskMemFree(Some(mix.cast()));
                if settings.exclusive {
                    let mut chosen = None;
                    let mut last_error = String::new();
                    let rates: &[u32] = if remote_audio::receiver() {
                        &[48000]
                    } else {
                        &[48000, 44100, 96000]
                    };
                    let formats: &[(u16, u32)] = if remote_audio::receiver() {
                        &[(3, 32), (1, 32), (1, 24), (1, 16)]
                    } else {
                        &[(1, 24), (1, 16), (3, 32), (1, 32)]
                    };
                    for &rate in rates {
                        for &(tag, bits) in formats {
                            for extensible in [true, false] {
                                let mut candidate = WAVEFORMATEXTENSIBLE::default();
                                candidate.Format = WAVEFORMATEX {
                                    wFormatTag: tag,
                                    nChannels: 2,
                                    nSamplesPerSec: rate,
                                    nAvgBytesPerSec: rate * 2 * bits / 8,
                                    nBlockAlign: (2 * bits / 8) as u16,
                                    wBitsPerSample: bits as u16,
                                    cbSize: 0,
                                };
                                if extensible {
                                    candidate.Format.wFormatTag = 65534;
                                    candidate.Format.cbSize = 22;
                                    candidate.Samples.wValidBitsPerSample = bits as u16;
                                    candidate.dwChannelMask = 3;
                                    candidate.SubFormat = if tag == 3 {
                                        windows::Win32::Media::Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
                                    } else {
                                        windows::Win32::Media::KernelStreaming::KSDATAFORMAT_SUBTYPE_PCM
                                    };
                                }
                                match initialize_client(&device, &candidate, true) {
                                    Ok(opened) => {
                                        chosen = Some((candidate, opened));
                                        break;
                                    }
                                    Err(error) => last_error = error,
                                }
                            }
                            if chosen.is_some() {
                                break;
                            }
                        }
                        if chosen.is_some() {
                            break;
                        }
                    }
                    let (chosen_format, opened) = chosen.ok_or_else(|| {
                        format!(
                            "无法打开独占输出（设备格式不支持、被占用或独占被禁用）：{last_error}"
                        )
                    })?;
                    format = chosen_format;
                    client = opened;
                } else {
                    client = initialize_client(&device, &format, false)?;
                }
                let f = format.Format;
                if remote_audio::receiver() && f.nSamplesPerSec != 48000 {
                    return Err("远程无损收听需要 48 kHz 输出，请在 Windows 声音设置中选择 48 kHz，或使用独占模式".into());
                }
                let subtype = format.SubFormat;
                let bits = f.wBitsPerSample;
                let float = f.wFormatTag == 3
                    || (f.wFormatTag == 65534
                        && subtype
                            == windows::Win32::Media::Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT);
                let pcm = f.wFormatTag == 1
                    || (f.wFormatTag == 65534
                        && subtype
                            == windows::Win32::Media::KernelStreaming::KSDATAFORMAT_SUBTYPE_PCM);
                if f.nSamplesPerSec < 8000
                    || f.nSamplesPerSec > 384000
                    || f.nBlockAlign != f.nChannels * (bits / 8)
                    || f.nChannels < 2
                    || f.nChannels > 32
                    || !(float && bits == 32 || pcm && [16, 24, 32].contains(&bits))
                {
                    return Err("unsupported output mix format".into());
                }
                let size = client.GetBufferSize().map_err(|e| e.to_string())?;
                // Match the event-driven shared WASAPI path used before endpoint
                // management. Driver buffer readiness, not timer polling, schedules writes.
                let event = if settings.exclusive {
                    None
                } else {
                    let event = OutputEvent(
                        windows::Win32::System::Threading::CreateEventW(None, false, false, None)
                            .map_err(|e| e.to_string())?,
                    );
                    client.SetEventHandle(event.0).map_err(|e| e.to_string())?;
                    Some(event)
                };
                let render: IAudioRenderClient = client.GetService().map_err(|e| e.to_string())?;
                Ok(Self {
                    client,
                    render,
                    id: endpoint_id(&device)?,
                    name: endpoint_name(&device),
                    settings: settings.clone(),
                    rate: f.nSamplesPerSec,
                    channels: f.nChannels as usize,
                    bits: f.wBitsPerSample,
                    float,
                    size,
                    converter: device_output::DeviceOutput::new(
                        f.nSamplesPerSec,
                        STEREO_FIFO_START_FRAMES,
                    ),
                    monitor: Default::default(),
                    fade: 0.0,
                    lossless: remote_audio::receiver(),
                    event,
                })
            }
        }
        fn start(&self) -> Result<(), String> {
            unsafe { self.client.Start().map_err(|e| e.to_string()) }
        }
        fn tick(
            &mut self,
            fifo: &stereo_fifo::StereoFifo,
            t: &RuntimeTelemetry,
            fade_out: bool,
        ) -> Result<(), String> {
            unsafe {
                if let Some(event) = &self.event {
                    let ready = windows::Win32::System::Threading::WaitForSingleObject(event.0, 0);
                    if ready == windows::Win32::Foundation::WAIT_FAILED {
                        return Err(windows::core::Error::from_win32().to_string());
                    }
                    if ready != windows::Win32::Foundation::WAIT_OBJECT_0 {
                        return Ok(());
                    }
                }
                let padding = self
                    .client
                    .GetCurrentPadding()
                    .map_err(|e| format!("GetCurrentPadding: {e}"))?;
                let count = self.size.saturating_sub(padding);
                if count == 0 {
                    return Ok(());
                }
                let ptr = self
                    .render
                    .GetBuffer(count)
                    .map_err(|e| format!("GetBuffer: {e}"))?;
                let started = Instant::now();
                if fifo.apply_flush_from_consumer() {
                    self.converter.reset();
                    self.monitor.reset();
                }
                let enabled = t.callback_output_enabled.load(Ordering::Acquire)
                    && remote_sync::output_allowed();
                let stride = self.channels * self.bits as usize / 8;
                let bytes = std::slice::from_raw_parts_mut(ptr, count as usize * stride);
                bytes.fill(0);
                let step = 1.0 / (self.rate as f32 * 0.005);
                let channels = self.channels;
                let bits = self.bits;
                let float = self.float;
                let fade = &mut self.fade;
                let lossless = self.lossless;
                let local_muted = remote_audio::local_muted();
                let monitor = &mut self.monitor;
                let sample_pos = t.callback_consumed_sample_pos.load(Ordering::Relaxed);
                let popped =
                    self.converter
                        .fill(fifo, enabled, count as usize, |i, frame| {
                            *fade = if lossless {
                                1.0
                            } else if fade_out || local_muted {
                                (*fade - step).max(0.0)
                            } else {
                                (*fade + step).min(1.0)
                            };
                            monitor.observe(
                                std::iter::once([frame[0] * *fade, frame[1] * *fade]),
                                sample_pos + (i as u64 * 48000 / self.rate as u64),
                                &t.output,
                            );
                            for ear in 0..2 {
                                let value = frame[ear] * *fade;
                                let offset = (i * channels + ear) * bits as usize / 8;
                                match (float, bits) {
                                    (true, 32) => bytes[offset..offset + 4]
                                        .copy_from_slice(&value.to_le_bytes()),
                                    (false, 16) => bytes[offset..offset + 2].copy_from_slice(
                                        &((value.clamp(-1.0, 1.0) * 32767.0).round() as i16)
                                            .to_le_bytes(),
                                    ),
                                    (false, 24) => bytes[offset..offset + 3].copy_from_slice(
                                        &((value.clamp(-1.0, 1.0) * 8388607.0).round() as i32)
                                            .to_le_bytes()[..3],
                                    ),
                                    (false, 32) => bytes[offset..offset + 4].copy_from_slice(
                                        &((value.clamp(-1.0, 1.0) as f64 * 2147483647.0).round()
                                            as i32)
                                            .to_le_bytes(),
                                    ),
                                    _ => {}
                                }
                            }
                        });
                self.render
                    .ReleaseBuffer(count, 0)
                    .map_err(|e| e.to_string())?;
                record_callback(t, started, self.converter.requested_source, popped, enabled);
                Ok(())
            }
        }
        fn status(&self, requested: &Settings, detail: String) -> Status {
            Status {
                requested: requested.clone(),
                actual_id: Some(self.id.clone()),
                actual_name: Some(self.name.clone()),
                mode: Some(
                    if self.settings.exclusive {
                        "exclusive"
                    } else {
                        "shared"
                    }
                    .into(),
                ),
                sample_rate: Some(self.rate),
                channels: Some(self.channels as u16),
                buffer_ms: Some(self.size as f64 * 1000.0 / self.rate as f64),
                sample_format: Some(format!(
                    "{}-bit {}",
                    self.bits,
                    if self.float { "float" } else { "PCM" }
                )),
                state: "ready".into(),
                detail,
            }
        }
    }

    // The manager owns exactly one FIFO consumer across WASAPI and ASIO.
    enum Backend {
        Wasapi(WasapiOutput),
        Asio(asio_output::AsioOutput),
        DirectSound(directsound_output::DirectSoundOutput),
    }
    struct Output {
        backend: Backend,
        id: String,
        settings: Settings,
        rate: u32,
        channels: usize,
    }
    impl Output {
        fn open(
            e: &IMMDeviceEnumerator,
            s: &Settings,
            fifo: &Arc<stereo_fifo::StereoFifo>,
            t: &Arc<RuntimeTelemetry>,
        ) -> Result<Self, String> {
            if s.device_id
                .as_deref()
                .is_some_and(|id| id.starts_with("asio:"))
            {
                let o = asio_output::AsioOutput::open(s, fifo.clone(), t.clone())?;
                Ok(Self {
                    id: s.device_id.clone().unwrap(),
                    settings: s.clone(),
                    rate: o.rate,
                    channels: o.channels,
                    backend: Backend::Asio(o),
                })
            } else if s
                .device_id
                .as_deref()
                .is_some_and(|id| id.starts_with("dsound:"))
            {
                let o = directsound_output::DirectSoundOutput::open(s)?;
                Ok(Self {
                    id: s.device_id.clone().unwrap(),
                    settings: s.clone(),
                    rate: 48000,
                    channels: 2,
                    backend: Backend::DirectSound(o),
                })
            } else {
                let o = WasapiOutput::open(e, s)?;
                Ok(Self {
                    id: o.id.clone(),
                    settings: o.settings.clone(),
                    rate: o.rate,
                    channels: o.channels,
                    backend: Backend::Wasapi(o),
                })
            }
        }
        fn start(&self) -> Result<(), String> {
            match &self.backend {
                Backend::Wasapi(o) => o.start(),
                Backend::Asio(o) => o.start(),
                Backend::DirectSound(o) => o.start(),
            }
        }
        fn tick(
            &mut self,
            fifo: &stereo_fifo::StereoFifo,
            t: &RuntimeTelemetry,
            fade: bool,
        ) -> Result<(), String> {
            match &mut self.backend {
                Backend::Wasapi(o) => o.tick(fifo, t, fade),
                Backend::Asio(o) => o.tick(),
                Backend::DirectSound(o) => o.tick(fifo, t, fade),
            }
        }
        fn status(&self, s: &Settings, d: String) -> Status {
            match &self.backend {
                Backend::Wasapi(o) => o.status(s, d),
                Backend::Asio(o) => o.status(s, d),
                Backend::DirectSound(o) => o.status(s, d),
            }
        }
        fn drain(&mut self, fifo: &stereo_fifo::StereoFifo, t: &RuntimeTelemetry) {
            match &mut self.backend {
                Backend::Asio(o) => o.drain(),
                Backend::DirectSound(o) => o.stop(),
                Backend::Wasapi(o) => {
                    let end = Instant::now() + Duration::from_millis(80);
                    while Instant::now() < end && o.fade > 0.0 {
                        if o.tick(fifo, t, true).is_err() {
                            break;
                        }
                        thread::sleep(Duration::from_millis(2));
                    }
                    while Instant::now() < end
                        && unsafe { o.client.GetCurrentPadding().unwrap_or(0) } > 0
                    {
                        thread::sleep(Duration::from_millis(2));
                    }
                }
            }
        }
    }

    fn remote_status(requested: &Settings) -> Status {
        Status {
            requested: requested.clone(),
            actual_id: None,
            actual_name: Some("一对一无损远程".into()),
            mode: Some("remote".into()),
            sample_rate: Some(48000),
            channels: Some(2),
            buffer_ms: None,
            sample_format: Some("32-bit float".into()),
            state: "ready".into(),
            detail: "32-bit float PCM · 远端时钟".into(),
        }
    }
    fn unavailable(requested: &Settings, detail: String) -> Status {
        Status {
            requested: requested.clone(),
            actual_id: None,
            actual_name: None,
            mode: None,
            sample_rate: None,
            channels: None,
            buffer_ms: None,
            sample_format: None,
            state: "unavailable".into(),
            detail,
        }
    }
    // Retry the requested endpoint even when the device snapshot has not changed.
    // Bluetooth can invalidate an IAudioClient while its endpoint remains ACTIVE.
    fn recover_output<T>(
        output: &mut Option<T>,
        requested: &Settings,
        last_retry: &mut Instant,
        open: impl FnOnce(&Settings) -> Result<T, String>,
    ) -> Option<Result<(), String>> {
        if output.is_some() || last_retry.elapsed() < Duration::from_secs(1) {
            return None;
        }
        *last_retry = Instant::now();
        Some(open(requested).map(|next| {
            *output = Some(next);
        }))
    }
    fn publish(status: Status, devices: Vec<Endpoint>) {
        write_event(&Event::OutputDevices { status, devices });
    }
    pub fn run(
        fifo: Arc<stereo_fifo::StereoFifo>,
        telemetry: Arc<RuntimeTelemetry>,
        commands: Arc<render_command::RenderCommandQueue>,
    ) {
        let _realtime = crate::realtime::ProAudio::enter();
        let _com = match ComScope::new() {
            Ok(c) => c,
            Err(detail) => {
                write_event(&Event::Error { detail });
                return;
            }
        };
        let _timer = if unsafe { windows::Win32::Media::timeBeginPeriod(1) } == 0 {
            Some(TimerScope)
        } else {
            None
        };
        let e = match enumerator() {
            Ok(e) => e,
            Err(detail) => {
                write_event(&Event::Error { detail });
                return;
            }
        };
        let (tx, rx) = mpsc::channel();
        let _ = CONTROL.set(tx.clone());
        let mut requested: Settings = std::env::var("SDA_OUTPUT_SETTINGS")
            .ok()
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        requested = requested.normalized();
        let mut output = None;
        let mut detail = String::new();
        match Output::open(&e, &requested, &fifo, &telemetry).and_then(|o| {
            o.start()?;
            Ok(o)
        }) {
            Ok(o) => output = Some(o),
            Err(err) => detail = err,
        }
        let mut devices = list(&e).unwrap_or_default();
        publish(
            output.as_ref().map_or_else(
                || unavailable(&requested, detail.clone()),
                |o| o.status(&requested, detail.clone()),
            ),
            devices.clone(),
        );
        write_event(&Event::Ready {
            protocol: PROTOCOL,
            sample_rate: 48000,
            output_channels: 2,
            features: &["programCodec"],
        });
        let input_fifo = fifo.clone();
        let input_t = telemetry.clone();
        thread::spawn(move || {
            if remote_audio::receiver() {
                if let Err(error) = remote_audio::read_receiver(input_fifo, input_t) {
                    write_event(&Event::Error {
                        detail: error.to_string(),
                    });
                }
            } else {
                let _ = protocol::read_frames(&mut io::stdin().lock(), &commands);
            }
            stop();
        });
        // Endpoint/property-store enumeration can stall a driver. Keep it away
        // from the audio pump; only immutable snapshots cross the COM boundary.
        thread::spawn(move || {
            let Ok(_com) = ComScope::new() else {
                return;
            };
            let Ok(e) = enumerator() else {
                return;
            };
            let mut previous = Vec::new();
            loop {
                if let Ok(devices) = list(&e) {
                    if devices != previous {
                        previous = devices.clone();
                        if tx.send(Request::Devices(devices)).is_err() {
                            break;
                        }
                    }
                }
                thread::sleep(Duration::from_secs(1));
            }
        });
        let mut remote: Option<remote_audio::HostOutput> = None;
        let mut remote_selected = false;
        let mut virtual_tick = Instant::now();
        let remote_telemetry = RuntimeTelemetry::default();
        let _ = remote_audio::mirror_fifo();
        let mut panel = false;
        let mut deferred = std::collections::VecDeque::new();
        let mut last_retry = Instant::now();
        loop {
            if panel && !asio_output::AsioOutput::pump_panel() {
                panel = false;
                write_event(&Event::Ack {
                    command: "openAsioControlPanel",
                    accepted: true,
                    detail: None,
                });
            }
            let request = if !panel && !deferred.is_empty() {
                Ok(deferred.pop_front().unwrap())
            } else {
                rx.recv_timeout(Duration::from_millis(2))
            };
            // Never destroy or reconfigure a driver while its control panel is active.
            let request = if panel {
                if let Ok(request) = request {
                    deferred.push_back(request);
                }
                Err(mpsc::RecvTimeoutError::Timeout)
            } else {
                request
            };
            match request {
                Ok(Request::ControlPanel) => {
                    let result = match output.as_ref().map(|o| &o.backend) {
                        Some(Backend::Asio(asio)) => asio.control_panel(),
                        _ => Err("请先应用 ASIO 输出设备".to_string()),
                    };
                    match result {
                        Ok(()) => panel = true,
                        Err(error) => write_event(&Event::Ack {
                            command: "openAsioControlPanel",
                            accepted: false,
                            detail: Some(&error),
                        }),
                    }
                }
                Ok(Request::Stop) => break,
                Ok(Request::Remote(next)) => {
                    let result = if let Some((address, token)) = next {
                        remote_audio::HostOutput::connect(&address, &token).map(|sink| {
                            remote = Some(sink);
                            remote_selected = true;
                            remote_audio::HOST_SELECTED.store(true, Ordering::Release);
                        })
                    } else {
                        remote = None;
                        remote_selected = false;
                        remote_audio::HOST_SELECTED.store(false, Ordering::Release);
                        remote_audio::select_mirror(false);
                        if output.is_some() {
                            Ok(())
                        } else {
                            Output::open(&e, &requested, &fifo, &telemetry).and_then(|o| {
                                o.start()?;
                                output = Some(o);
                                Ok(())
                            })
                        }
                    };
                    let accepted = result.is_ok();
                    let error = result.err();
                    write_event(&Event::Ack {
                        command: "setRemoteOutput",
                        accepted,
                        detail: error.as_deref(),
                    });
                    let status = output.as_ref().map_or_else(
                        || {
                            if remote_selected {
                                remote_status(&requested)
                            } else {
                                unavailable(&requested, error.unwrap_or_default())
                            }
                        },
                        |o| {
                            o.status(
                                &requested,
                                if remote_selected {
                                    "本机与远端同时输出".into()
                                } else {
                                    String::new()
                                },
                            )
                        },
                    );
                    publish(status, devices.clone());
                }
                Ok(Request::List) => {
                    publish(
                        output.as_ref().map_or_else(
                            || unavailable(&requested, detail.clone()),
                            |o| o.status(&requested, detail.clone()),
                        ),
                        devices.clone(),
                    );
                    write_event(&Event::Ack {
                        command: "listOutputDevices",
                        accepted: true,
                        detail: None,
                    });
                }
                Ok(Request::Set(next)) => {
                    // Drain a bounded fade before closing the old endpoint. No render
                    // graph/session reset, decoder restart or source reconfiguration.
                    if let Some(old) = &mut output {
                        old.drain(&fifo, &telemetry);
                    }
                    let previous = output.as_ref().map(|o| o.settings.clone());
                    drop(output.take());
                    let result = Output::open(&e, &next, &fifo, &telemetry).and_then(|o| {
                        o.start()?;
                        Ok(o)
                    });
                    let accepted = result.is_ok();
                    match result {
                        Ok(o) => {
                            output = Some(o);
                            requested = next;
                            detail.clear();
                        }
                        Err(err) => {
                            detail = err;
                            if let Some(previous) = previous {
                                output = Output::open(&e, &previous, &fifo, &telemetry)
                                    .and_then(|o| {
                                        o.start()?;
                                        Ok(o)
                                    })
                                    .ok();
                            }
                        }
                    }
                    publish(
                        output.as_ref().map_or_else(
                            || unavailable(&requested, detail.clone()),
                            |o| o.status(&requested, detail.clone()),
                        ),
                        devices.clone(),
                    );
                    write_event(&Event::Ack {
                        command: "setOutputDevice",
                        accepted,
                        detail: if accepted { None } else { Some(&detail) },
                    });
                }
                Ok(Request::Devices(next)) => {
                    if next != devices {
                        devices = next;
                        let wanted = requested.device_id.clone().or_else(|| {
                            devices
                                .iter()
                                .find(|d| d.is_default && d.available)
                                .map(|d| d.id.clone())
                        });
                        let actual = output.as_ref().map(|o| o.id.clone());
                        // Remote/session drivers can change their shared mix format
                        // without changing endpoint ID (for example after reconnect).
                        let format_changed = output.as_ref().is_some_and(|o| {
                            !o.settings.exclusive
                                && !o.id.starts_with("asio:")
                                && !o.id.starts_with("dsound:")
                                && devices.iter().any(|d| {
                                    d.id == o.id
                                        && d.available
                                        && (d.sample_rate.is_some_and(|r| r != o.rate)
                                            || d.channels.is_some_and(|c| c as usize != o.channels))
                                })
                        });
                        if (actual != wanted && requested.device_id.is_none()) || format_changed {
                            drop(output.take());
                            match Output::open(&e, &requested, &fifo, &telemetry).and_then(|o| {
                                o.start()?;
                                Ok(o)
                            }) {
                                Ok(o) => {
                                    output = Some(o);
                                    detail.clear();
                                }
                                Err(err) => detail = err,
                            }
                        }
                        if output
                            .as_ref()
                            .is_some_and(|o| !devices.iter().any(|d| d.id == o.id && d.available))
                        {
                            drop(output.take());
                            detail = "所选输出设备不可用，等待恢复后重新打开音频输出".into();
                        }
                        publish(
                            output.as_ref().map_or_else(
                                || unavailable(&requested, detail.clone()),
                                |o| o.status(&requested, detail.clone()),
                            ),
                            devices.clone(),
                        );
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                _ => {}
            }
            let synchronized = remote_sync::ENABLED.load(Ordering::Acquire);
            remote_audio::select_mirror(remote.is_some() && (output.is_some() || synchronized));
            if output.is_none() && synchronized {
                fifo.apply_flush_from_consumer();
                if virtual_tick.elapsed() >= Duration::from_millis(10) {
                    let frames =
                        ((virtual_tick.elapsed().as_secs_f64() * 48000.0) as usize).min(4800);
                    virtual_tick = Instant::now();
                    if telemetry.callback_output_enabled.load(Ordering::Acquire)
                        && remote_sync::output_allowed()
                    {
                        let popped = fifo.pop_frames(frames, |_, _| {});
                        telemetry
                            .callback_consumed_sample_pos
                            .fetch_add(popped as u64, Ordering::Release);
                    }
                }
            } else {
                virtual_tick = Instant::now();
            }
            if let Some(sink) = &mut remote {
                remote_telemetry.callback_output_enabled.store(
                    telemetry.callback_output_enabled.load(Ordering::Acquire),
                    Ordering::Release,
                );
                let result = if output.is_some() || synchronized {
                    // A new connection must wait for the render worker to reset the
                    // mirror epoch before inspecting the previous session's overflow.
                    if !remote_audio::mirror_ready() {
                        Ok(())
                    } else if remote_audio::mirror_overflow() {
                        Err("远程接收超过 8 秒未跟上播放，请重新连接".into())
                    } else {
                        sink.tick_positioned(
                            remote_audio::mirror_fifo(),
                            &remote_telemetry,
                            remote_audio::mirror_origin(),
                        )
                    }
                } else {
                    sink.tick(&fifo, &telemetry)
                };
                if let Err(error) = result {
                    remote = None;
                    remote_selected = false;
                    remote_audio::HOST_SELECTED.store(false, Ordering::Release);
                    remote_audio::select_mirror(false);
                    publish(
                        unavailable(&requested, format!("远程连接中断：{error}")),
                        devices.clone(),
                    );
                    write_event(&Event::Error {
                        detail: format!("remote audio: {error}"),
                    });
                }
            }
            if panel {
                continue;
            }
            if let Some(active) = &mut output {
                if let Err(err) = active.tick(&fifo, &telemetry, false) {
                    detail = format!("音频输出会话失效，正在重新初始化：{err}");
                    last_retry = Instant::now();
                    drop(output.take());
                    publish(unavailable(&requested, detail.clone()), devices.clone());
                }
            }
            if let Some(result) =
                recover_output(&mut output, &requested, &mut last_retry, |settings| {
                    Output::open(&e, settings, &fifo, &telemetry).and_then(|o| {
                        o.start()?;
                        Ok(o)
                    })
                })
            {
                match result {
                    Ok(()) => {
                        detail.clear();
                        publish(
                            output.as_ref().unwrap().status(&requested, detail.clone()),
                            devices.clone(),
                        );
                    }
                    Err(err) => {
                        let next = format!("音频输出暂不可用，将自动重新初始化：{err}");
                        if detail != next {
                            detail = next;
                            publish(unavailable(&requested, detail.clone()), devices.clone());
                        }
                    }
                }
            }
        }
        // Keep the device running until its queue contains silence. In particular,
        // dropping ASIO immediately can replay the old downstream tail on reopen.
        telemetry
            .callback_output_enabled
            .store(false, Ordering::Release);
        if let Some(active) = &mut output {
            match &mut active.backend {
                Backend::Asio(asio) => asio.drain_shutdown(),
                _ => active.drain(&fifo, &telemetry),
            }
        }
        drop(output);
        drop(e);
    }

    #[cfg(test)]
    mod recovery_tests {
        use super::*;
        #[test]
        fn asio_settings_preserve_driver_and_remote_mode_clears_it() {
            let s = Settings {
                device_id: Some("asio:ASIO4ALL v2".into()),
                exclusive: true,
                remote_compatible: false,
            }
            .normalized();
            assert_eq!(s.device_id.as_deref(), Some("asio:ASIO4ALL v2"));
            assert!(!s.exclusive);
            let remote = Settings {
                remote_compatible: true,
                ..s
            }
            .normalized();
            assert_eq!(remote.device_id, None);
            assert!(!remote.exclusive);
        }
        #[test]
        fn pinned_endpoint_recovers_without_device_change_or_fallback() {
            let settings = Settings {
                device_id: Some("airpods".into()),
                exclusive: true,
                remote_compatible: false,
            };
            let mut output: Option<String> = None;
            let mut last = Instant::now() - Duration::from_secs(2);
            let result = recover_output(&mut output, &settings, &mut last, |next| {
                assert_eq!(next, &settings);
                Err("device invalidated".into())
            });
            assert!(result.unwrap().is_err());
            assert!(output.is_none());
            assert!(
                recover_output(&mut output, &settings, &mut last, |_| panic!(
                    "retry must be bounded"
                ))
                .is_none()
            );
            last = Instant::now() - Duration::from_secs(2);
            assert!(
                recover_output(&mut output, &settings, &mut last, |next| Ok(next
                    .device_id
                    .clone()
                    .unwrap()))
                .unwrap()
                .is_ok()
            );
            assert_eq!(output.as_deref(), Some("airpods"));
            last = Instant::now() - Duration::from_secs(2);
            assert!(
                recover_output(&mut output, &settings, &mut last, |_| panic!(
                    "healthy output must remain open"
                ))
                .is_none()
            );
        }
        #[test]
        fn passive_format_property_handles_short_and_invalid_data() {
            assert_eq!(endpoint_format(&[0; 7]), None);
            let mut bytes = [0u8; 18];
            bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
            bytes[4..8].copy_from_slice(&48000u32.to_le_bytes());
            assert_eq!(endpoint_format(&bytes), Some((48000, 2)));
            bytes[2..4].copy_from_slice(&0u16.to_le_bytes());
            assert_eq!(endpoint_format(&bytes), None);
        }
        #[test]
        fn default_output_still_recovers() {
            let mut output = None;
            let mut last = Instant::now() - Duration::from_secs(2);
            assert!(
                recover_output(&mut output, &Settings::default(), &mut last, |next| {
                    assert!(next.device_id.is_none());
                    Ok(())
                })
                .unwrap()
                .is_ok()
            );
        }
    }
} // end #[cfg(windows)] mod platform

// ── macOS / Linux CPAL implementation ───────────────────────────────────────
#[cfg(not(windows))]
mod platform {
    use super::super::record_callback;
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::{Arc, Mutex};

    fn list_devices(host: &cpal::Host) -> Vec<Endpoint> {
        let default_name = host
            .default_output_device()
            .and_then(|d| d.name().ok())
            .unwrap_or_default();
        let mut endpoints = Vec::new();
        if let Ok(devices) = host.output_devices() {
            for device in devices {
                let name = device.name().unwrap_or_else(|_| "Audio output".into());
                let is_default = name == default_name;
                let (sample_rate, channels) = device
                    .default_output_config()
                    .ok()
                    .map(|c| (c.sample_rate().0, c.channels()))
                    .unzip();
                endpoints.push(Endpoint {
                    id: name.clone(),
                    name,
                    available: true,
                    is_default,
                    sample_rate,
                    channels: Some(channels.unwrap_or(2)),
                });
            }
        }
        endpoints.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
        endpoints
    }

    fn status_ready(requested: &Settings, name: &str, rate: u32, channels: u16) -> Status {
        Status {
            requested: requested.clone(),
            actual_id: Some(name.into()),
            actual_name: Some(name.into()),
            mode: Some("shared".into()),
            sample_rate: Some(rate),
            channels: Some(channels),
            buffer_ms: None,
            sample_format: Some("32-bit float".into()),
            state: "ready".into(),
            detail: format!("{rate} Hz · {channels}ch · f32"),
        }
    }

    fn status_unavailable(requested: &Settings, detail: &str) -> Status {
        Status {
            requested: requested.clone(),
            actual_id: None,
            actual_name: None,
            mode: None,
            sample_rate: None,
            channels: None,
            buffer_ms: None,
            sample_format: None,
            state: "unavailable".into(),
            detail: detail.into(),
        }
    }

    fn publish(status: Status, devices: Vec<Endpoint>) {
        write_event(&Event::OutputDevices { status, devices });
    }

    struct CpalOutput {
        stream: cpal::Stream,
        device_name: String,
        rate: u32,
        channels: u16,
        converter: device_output::DeviceOutput,
        monitor: output_monitor::OutputMonitor,
        fade: f32,
        settings: Settings,
    }

    struct SharedState {
        fifo: Arc<stereo_fifo::StereoFifo>,
        telemetry: Arc<RuntimeTelemetry>,
        converter: Option<device_output::DeviceOutput>,
        monitor: output_monitor::OutputMonitor,
        fade: f32,
        rate: u32,
        lossless: bool,
    }

    fn open_device(
        host: &cpal::Host,
        settings: &Settings,
        fifo: Arc<stereo_fifo::StereoFifo>,
        telemetry: Arc<RuntimeTelemetry>,
    ) -> Result<CpalOutput, String> {
        let device = if let Some(id) = &settings.device_id {
            host.output_devices()
                .map_err(|e| e.to_string())?
                .find(|d| d.name().ok().as_deref() == Some(id))
                .ok_or_else(|| format!("output device not found: {id}"))?
        } else {
            host.default_output_device()
                .ok_or_else(|| "no default output device".to_string())?
        };
        let name = device.name().unwrap_or_else(|_| "Audio output".into());
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels();
        let config = cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };
        let lossless = remote_audio::receiver();
        let shared = Arc::new(Mutex::new(SharedState {
            fifo,
            telemetry,
            converter: Some(device_output::DeviceOutput::new(
                sample_rate,
                STEREO_FIFO_START_FRAMES,
            )),
            monitor: output_monitor::OutputMonitor::default(),
            fade: 0.0,
            rate: sample_rate,
            lossless,
        }));
        let shared_cb = shared.clone();
        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                    let Ok(mut state) = shared_cb.lock() else {
                        return;
                    };
                    let SharedState {
                        fifo,
                        telemetry,
                        converter,
                        monitor,
                        fade,
                        rate,
                        lossless,
                    } = &mut *state;
                    let Some(conv) = converter.as_mut() else {
                        data.fill(0.0);
                        return;
                    };
                    // Always acknowledge pending FIFO flushes so the render worker
                    // can proceed after an epoch change, even when output is disabled.
                    fifo.apply_flush_from_consumer();
                    let enabled = telemetry.callback_output_enabled.load(Ordering::Acquire)
                        && remote_sync::output_allowed();
                    let ch = channels as usize;
                    let sample_pos = telemetry
                        .callback_consumed_sample_pos
                        .load(Ordering::Relaxed);
                    let step = 1.0 / (*rate as f32 * 0.005);
                    let _local_muted = remote_audio::local_muted();
                    let rate_f = *rate;
                    let lossless_flag = *lossless;
                    conv.fill(fifo, enabled, data.len() / ch, |i, frame| {
                        *fade = if lossless_flag {
                            1.0
                        } else {
                            (*fade + step).min(1.0)
                        };
                        monitor.observe(
                            std::iter::once([frame[0] * *fade, frame[1] * *fade]),
                            sample_pos + (i as u64 * 48000 / rate_f as u64),
                            &telemetry.output,
                        );
                        for ear in 0..2.min(ch) {
                            data[i * ch + ear] = frame[ear] * *fade;
                        }
                        // Zero extra channels beyond stereo
                        for c in 2..ch {
                            data[i * ch + c] = 0.0;
                        }
                    });
                    let started = Instant::now();
                    let popped = conv.requested_source;
                    record_callback(&telemetry, started, conv.requested_source, popped, enabled);
                },
                |err| {
                    eprintln!("[SDA native] output stream error: {err}");
                },
                None,
            )
            .map_err(|e| format!("failed to build output stream: {e}"))?;
        stream
            .play()
            .map_err(|e| format!("failed to start output stream: {e}"))?;
        Ok(CpalOutput {
            stream,
            device_name: name,
            rate: sample_rate,
            channels,
            converter: device_output::DeviceOutput::new(sample_rate, STEREO_FIFO_START_FRAMES),
            monitor: output_monitor::OutputMonitor::default(),
            fade: 0.0,
            settings: settings.clone(),
        })
    }

    pub fn run(
        fifo: Arc<stereo_fifo::StereoFifo>,
        telemetry: Arc<RuntimeTelemetry>,
        commands: Arc<render_command::RenderCommandQueue>,
    ) {
        let host = cpal::default_host();
        let (tx, rx) = mpsc::channel();
        let _ = CONTROL.set(tx.clone());
        let mut requested: Settings = std::env::var("SDA_OUTPUT_SETTINGS")
            .ok()
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        requested = requested.normalized();
        let mut output: Option<CpalOutput> = None;
        let mut detail = String::new();
        // Try opening default device
        match open_device(&host, &requested, fifo.clone(), telemetry.clone()) {
            Ok(o) => output = Some(o),
            Err(err) => detail = err,
        }
        let mut devices = list_devices(&host);
        publish(
            output.as_ref().map_or_else(
                || status_unavailable(&requested, &detail),
                |o| status_ready(&requested, &o.device_name, o.rate, o.channels),
            ),
            devices.clone(),
        );
        write_event(&Event::Ready {
            protocol: PROTOCOL,
            sample_rate: 48000,
            output_channels: 2,
            features: &["programCodec"],
        });
        let input_fifo = fifo.clone();
        let input_t = telemetry.clone();
        thread::spawn(move || {
            if remote_audio::receiver() {
                if let Err(error) = remote_audio::read_receiver(input_fifo, input_t) {
                    write_event(&Event::Error {
                        detail: error.to_string(),
                    });
                }
            } else {
                let _ = protocol::read_frames(&mut io::stdin().lock(), &commands);
            }
            stop();
        });
        // Device polling thread
        let tx_poll = tx.clone();
        thread::spawn(move || {
            let mut previous = Vec::new();
            loop {
                let devices = list_devices(&host);
                if devices != previous {
                    previous = devices.clone();
                    if tx_poll.send(Request::Devices(devices)).is_err() {
                        break;
                    }
                }
                thread::sleep(Duration::from_secs(1));
            }
        });
        let mut remote: Option<remote_audio::HostOutput> = None;
        let mut remote_selected = false;
        let mut virtual_tick = Instant::now();
        let remote_telemetry = RuntimeTelemetry::default();
        let _ = remote_audio::mirror_fifo();
        let mut last_retry = Instant::now();
        loop {
            let request = rx.recv_timeout(Duration::from_millis(10));
            match request {
                Ok(Request::ControlPanel) => {
                    write_event(&Event::Ack {
                        command: "openAsioControlPanel",
                        accepted: false,
                        detail: Some("ASIO is only supported on Windows"),
                    });
                }
                Ok(Request::Stop) => break,
                Ok(Request::Remote(next)) => {
                    let result = if let Some((address, token)) = next {
                        remote_audio::HostOutput::connect(&address, &token).map(|sink| {
                            remote = Some(sink);
                            remote_selected = true;
                            remote_audio::HOST_SELECTED.store(true, Ordering::Release);
                        })
                    } else {
                        remote = None;
                        remote_selected = false;
                        remote_audio::HOST_SELECTED.store(false, Ordering::Release);
                        remote_audio::select_mirror(false);
                        Ok(())
                    };
                    let accepted = result.is_ok();
                    let error = result.err();
                    write_event(&Event::Ack {
                        command: "setRemoteOutput",
                        accepted,
                        detail: error.as_deref(),
                    });
                    let status = output.as_ref().map_or_else(
                        || {
                            if remote_selected {
                                status_unavailable(&requested, "remote output")
                            } else {
                                status_unavailable(&requested, &error.unwrap_or_default())
                            }
                        },
                        |o| status_ready(&requested, &o.device_name, o.rate, o.channels),
                    );
                    publish(status, devices.clone());
                }
                Ok(Request::List) => {
                    publish(
                        output.as_ref().map_or_else(
                            || status_unavailable(&requested, &detail),
                            |o| status_ready(&requested, &o.device_name, o.rate, o.channels),
                        ),
                        devices.clone(),
                    );
                    write_event(&Event::Ack {
                        command: "listOutputDevices",
                        accepted: true,
                        detail: None,
                    });
                }
                Ok(Request::Set(next)) => {
                    drop(output.take());
                    match open_device(
                        &cpal::default_host(),
                        &next,
                        fifo.clone(),
                        telemetry.clone(),
                    ) {
                        Ok(o) => {
                            output = Some(o);
                            requested = next;
                            detail.clear();
                        }
                        Err(err) => {
                            detail = err;
                        }
                    }
                    let accepted = output.is_some();
                    publish(
                        output.as_ref().map_or_else(
                            || status_unavailable(&requested, &detail),
                            |o| status_ready(&requested, &o.device_name, o.rate, o.channels),
                        ),
                        devices.clone(),
                    );
                    write_event(&Event::Ack {
                        command: "setOutputDevice",
                        accepted,
                        detail: if accepted { None } else { Some(&detail) },
                    });
                }
                Ok(Request::Devices(next)) => {
                    if next != devices {
                        devices = next;
                        let wanted = requested.device_id.clone().or_else(|| {
                            devices
                                .iter()
                                .find(|d| d.is_default && d.available)
                                .map(|d| d.id.clone())
                        });
                        let actual = output.as_ref().map(|o| o.device_name.clone());
                        if actual != wanted && requested.device_id.is_none() {
                            drop(output.take());
                            match open_device(
                                &cpal::default_host(),
                                &requested,
                                fifo.clone(),
                                telemetry.clone(),
                            ) {
                                Ok(o) => {
                                    output = Some(o);
                                    detail.clear();
                                }
                                Err(err) => detail = err,
                            }
                        }
                        if output.as_ref().is_some_and(|o| {
                            !devices.iter().any(|d| d.id == o.device_name && d.available)
                        }) {
                            drop(output.take());
                            detail = "selected output device unavailable".into();
                        }
                        publish(
                            output.as_ref().map_or_else(
                                || status_unavailable(&requested, &detail),
                                |o| status_ready(&requested, &o.device_name, o.rate, o.channels),
                            ),
                            devices.clone(),
                        );
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                _ => {}
            }
            // Remote audio tick
            let synchronized = remote_sync::ENABLED.load(Ordering::Acquire);
            remote_audio::select_mirror(remote.is_some() && (output.is_some() || synchronized));
            if output.is_none() && synchronized {
                fifo.apply_flush_from_consumer();
                if virtual_tick.elapsed() >= Duration::from_millis(10) {
                    let frames =
                        ((virtual_tick.elapsed().as_secs_f64() * 48000.0) as usize).min(4800);
                    virtual_tick = Instant::now();
                    if telemetry.callback_output_enabled.load(Ordering::Acquire)
                        && remote_sync::output_allowed()
                    {
                        let popped = fifo.pop_frames(frames, |_, _| {});
                        telemetry
                            .callback_consumed_sample_pos
                            .fetch_add(popped as u64, Ordering::Release);
                    }
                }
            } else {
                virtual_tick = Instant::now();
            }
            if let Some(sink) = &mut remote {
                remote_telemetry.callback_output_enabled.store(
                    telemetry.callback_output_enabled.load(Ordering::Acquire),
                    Ordering::Release,
                );
                let result = if output.is_some() || synchronized {
                    if !remote_audio::mirror_ready() {
                        Ok(())
                    } else if remote_audio::mirror_overflow() {
                        Err("remote receiver overflow".into())
                    } else {
                        sink.tick_positioned(
                            remote_audio::mirror_fifo(),
                            &remote_telemetry,
                            remote_audio::mirror_origin(),
                        )
                    }
                } else {
                    sink.tick(&fifo, &telemetry)
                };
                if let Err(error) = result {
                    remote = None;
                    remote_selected = false;
                    remote_audio::HOST_SELECTED.store(false, Ordering::Release);
                    remote_audio::select_mirror(false);
                    publish(
                        status_unavailable(&requested, &format!("remote disconnected: {error}")),
                        devices.clone(),
                    );
                    write_event(&Event::Error {
                        detail: format!("remote audio: {error}"),
                    });
                }
            }
            // Reconnect if output was lost
            if output.is_none() && last_retry.elapsed() >= Duration::from_secs(1) {
                last_retry = Instant::now();
                if let Ok(o) = open_device(
                    &cpal::default_host(),
                    &requested,
                    fifo.clone(),
                    telemetry.clone(),
                ) {
                    output = Some(o);
                    detail.clear();
                    let o = output.as_ref().unwrap();
                    publish(
                        status_ready(&requested, &o.device_name, o.rate, o.channels),
                        devices.clone(),
                    );
                }
            }
        }
        drop(output);
    }
}

pub use platform::run;
