use super::*;
const FIXTURE: &[u8] =
    include_bytes!("../../../packages/core/tests/fixtures/av3a-7-1-4-three-frames.av3a");
unsafe fn value(ptr: *mut c_char) -> Value {
    assert!(!ptr.is_null());
    let result: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(ptr) }.to_str().unwrap()).unwrap();
    unsafe {
        sda_ios_string_free(ptr);
    }
    assert_eq!(result["ok"], true, "{result}");
    result["value"].clone()
}
#[test]
fn av3a_system_speakers_match_shared_pcm_without_hrtf_or_channel_swaps() {
    let mut error = std::ptr::null_mut();
    let handle = unsafe { sda_ios_av3a_speakers_create(&mut error) };
    assert!(!handle.is_null());
    assert!(error.is_null());
    let mut actual = vec![];
    let mut output = vec![0.0; 377 * 12];
    for bytes in FIXTURE.chunks(173) {
        let status = unsafe {
            value(sda_ios_speakers_feed(
                handle,
                bytes.as_ptr(),
                bytes.len(),
                false,
            ))
        };
        assert_eq!(status["channels"], 12);
        assert_eq!(status["sampleRate"], 48000);
        loop {
            let n = unsafe { sda_ios_speakers_read(handle, output.as_mut_ptr(), 377) };
            if n == 0 {
                break;
            }
            actual.extend_from_slice(&output[..n * 12]);
        }
    }
    unsafe {
        value(sda_ios_speakers_feed(handle, std::ptr::null(), 0, true));
    }
    loop {
        let n = unsafe { sda_ios_speakers_read(handle, output.as_mut_ptr(), 377) };
        if n == 0 {
            break;
        }
        actual.extend_from_slice(&output[..n * 12]);
    }
    assert!(unsafe { (*handle.cast::<SpeakerHost>()).decoder.is_none() });
    assert_eq!(
        unsafe { value(sda_ios_speakers_objects(handle, 3344)) },
        serde_json::json!({})
    );
    unsafe {
        sda_ios_speakers_close(handle);
    }
    let mut decoder = sda_core::StreamingDecoder::new("av3a").unwrap();
    decoder.push(FIXTURE).unwrap();
    decoder.flush();
    assert!(decoder.drain_errors().is_empty());
    let mut expected = vec![];
    while let Some(f) = decoder.next_frame() {
        for i in 0..f.channels[0].len() {
            for ch in &f.channels {
                expected.push(ch[i]);
            }
        }
    }
    assert_eq!(actual.len(), 3344 * 12);
    assert_eq!(actual, expected);
}
#[test]
fn av3a_system_balance_is_one_protective_gain_for_all_twelve_channels() {
    let mut error = std::ptr::null_mut();
    let handle = unsafe { sda_ios_av3a_speakers_create(&mut error) };
    assert!(!handle.is_null());
    unsafe {
        sda_ios_speakers_balance(handle, true);
    }
    let measurement =
        CString::new(r#"{"integratedLufs":-5.0,"blocks":57,"truePeakDbtp":3.0}"#).unwrap();
    unsafe {
        value(sda_ios_speakers_measured(handle, measurement.as_ptr()));
    }
    let host = unsafe { &mut *handle.cast::<SpeakerHost>() };
    assert!(!host.measure_only);
    assert!(host.decoder.is_none());
    let input: Vec<f32> = (0..12).map(|ch| 0.01 * (ch + 1) as f32).collect();
    for _ in 0..128 {
        host.pcm.extend(input.iter().copied());
    }
    let mut out = vec![0.0; 128 * 12];
    assert_eq!(
        unsafe { sda_ios_speakers_read(handle, out.as_mut_ptr(), 128) },
        128
    );
    let gain = 10f32.powf(-13.0 / 20.0);
    for frame in out.chunks_exact(12) {
        for ch in 0..12 {
            assert!((frame[ch] - input[ch] * gain).abs() < 1e-7);
        }
    }
    unsafe {
        sda_ios_speakers_balance(handle, false);
    }
    let host = unsafe { &mut *handle.cast::<SpeakerHost>() };
    assert!(host.fixed_measurement.is_some());
    assert!((host.target_gain - gain).abs() < 1e-6);
    for _ in 0..12000 {
        host.pcm.extend(input.iter().copied());
    }
    let mut tail = vec![0.0; 1024 * 12];
    let mut last = vec![];
    loop {
        let n = unsafe { sda_ios_speakers_read(handle, tail.as_mut_ptr(), 1024) };
        if n == 0 {
            break;
        }
        last = tail[(n - 1) * 12..n * 12].to_vec();
    }
    for ch in 0..12 {
        assert!((last[ch] - input[ch]).abs() < 1e-7);
    }
    unsafe {
        sda_ios_speakers_close(handle);
    }
}
