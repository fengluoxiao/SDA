//! Standalone diagnostic only: invokes the production backend without audio output.
use super::{Batch, Shape, coordinator::Backend};
use std::time::Instant;
pub(super) fn probe_legacy(directory: &str) -> Result<serde_json::Value, String> {
    let mut backend = super::hexagon::Backend::open(std::path::Path::new(directory))?;
    let mut results = vec![];
    for (rows, parts, bins) in [
        (8, 1, 17),
        (8, 2, 17),
        (8, 3, 17),
        (8, 4, 17),
        (32, 2, 1025),
    ] {
        let shape = Shape::bounded(rows, parts, bins).ok_or("diagnostic shape bounds")?;
        let started = Instant::now();
        let name = backend.configure(shape)?;
        let configure_us = started.elapsed().as_micros() as u64;
        let mut batch = Batch::new(shape);
        let mut cpu_us = 0;
        let mut dsp_us = 0;
        for step in 0..10 {
            for (i, v) in batch.input.iter_mut().enumerate() {
                *v = (((i + step * 17) * 7919 % 1021) as f32 - 510.) / 511.;
            }
            let started = Instant::now();
            let n = shape.plane();
            batch.reference.fill(0.);
            for p in 0..parts {
                let v = &batch.input[p * 6 * n..];
                for i in 0..n {
                    for ear in 0..2 {
                        let h = v[(2 + 2 * ear) * n + i];
                        let k = v[(3 + 2 * ear) * n + i];
                        batch.reference[2 * ear * n + i] += v[i] * h - v[n + i] * k;
                        batch.reference[(2 * ear + 1) * n + i] += v[i] * k + v[n + i] * h;
                    }
                }
            }
            let cpu = started.elapsed().as_micros() as u64;
            let original = batch.input.to_vec();
            let started = Instant::now();
            backend.execute(&mut batch, 1_000_000)?;
            let dsp = started.elapsed().as_micros() as u64;
            if batch.input[..] != original[..] {
                return Err("legacy-DSP-mutated-input".into());
            }
            if !batch.reference_matches() {
                return Err(format!(
                    "legacy-DSP-precision-rejected:rows={rows},parts={parts},bins={bins},step={step}"
                ));
            }
            if step >= 2 {
                cpu_us += cpu;
                dsp_us += dsp;
            }
        }
        results.push(serde_json::json!({"rows":rows,"parts":parts,"bins":bins,"device":name,"configureMicros":configure_us,
            "lastDspExecutionCycles":backend.diagnostic_cycles(),"meanScalarReferenceMicros":cpu_us/8,"meanDspCallMicros":dsp_us/8,"precisionGate":"pass","inputUnchanged":true}));
    }
    Ok(
        serde_json::json!({"backend":backend.details(),"cases":results,"actualDspGraphExecuted":true,
        "audioOutputProduced":false,"playbackEndToEndQualified":false,"note":"Standalone RPC diagnostic; actual playback still requires full-block qualification"}),
    )
}
