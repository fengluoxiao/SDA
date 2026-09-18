//! Record an explicitly selected render endpoint via WASAPI loopback.
use std::{
    io::Write,
    time::{Duration, Instant},
};
use windows::{
    Win32::{Media::Audio::*, System::Com::*},
    core::{Interface, PCWSTR},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let check = args.len() == 3 && args[2] == "--check";
    if args.len() != 4 && !check {
        return Err("loopback <endpoint ID> <seconds 1..60> <raw output>".into());
    }
    let seconds: u64 = if check { 1 } else { args[2].parse()? };
    if !(1..=60).contains(&seconds) {
        return Err("invalid duration".into());
    }
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let id: Vec<u16> = args[1].encode_utf16().chain(Some(0)).collect();
        let d = e.GetDevice(PCWSTR(id.as_ptr()))?;
        let c: IAudioClient = d.Activate(CLSCTX_ALL, None)?;
        let f = c.GetMixFormat()?;
        let channels = (*f).nChannels;
        let rate = (*f).nSamplesPerSec;
        let bits = (*f).wBitsPerSample;
        let block = (*f).nBlockAlign;
        eprintln!(
            "Loopback format: channels={channels} rate={rate} bits={bits} tag={} block={block}",
            {
                let tag = (*f).wFormatTag;
                tag
            }
        );
        let initialized = c.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            0,
            0,
            f,
            None,
        );
        CoTaskMemFree(Some(f.cast()));
        if check {
            let code = initialized.as_ref().err().map(|e| e.code().0 as u32).unwrap_or(0);
            let mut pids = Vec::new();
            if let Ok(manager) = d.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) {
                if let Ok(sessions) = manager.GetSessionEnumerator() {
                    for i in 0..sessions.GetCount()? {
                        let s = sessions.GetSession(i)?;
                        if s.GetState()? == AudioSessionStateActive {
                            let control: IAudioSessionControl2 = s.cast()?;
                            let pid = control.GetProcessId()?;
                            if pid != 0 && !pids.contains(&pid) { pids.push(pid); }
                        }
                    }
                }
            }
            println!("{{\"code\":{code},\"pids\":{pids:?}}}");
            return Ok(());
        }
        initialized?;
        let capture: IAudioCaptureClient = c.GetService()?;
        let mut out = std::fs::File::create(&args[3])?;
        c.Start()?;
        let end = Instant::now() + Duration::from_secs(seconds);
        let mut total = 0u64;
        let mut nonzero = 0u64;
        while Instant::now() < end {
            while capture.GetNextPacketSize()? > 0 {
                let mut data = std::ptr::null_mut();
                let mut frames = 0;
                let mut flags = 0;
                capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let len = frames as usize * block as usize;
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                    out.write_all(&vec![0; len])?;
                } else {
                    let samples = std::slice::from_raw_parts(data, len);
                    nonzero += samples.iter().filter(|&&b| b != 0).count() as u64;
                    out.write_all(samples)?;
                }
                total += u64::from(frames);
                capture.ReleaseBuffer(frames)?;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        c.Stop()?;
        eprintln!("Captured frames={total} nonzeroBytes={nonzero}");
    }
    Ok(())
}
