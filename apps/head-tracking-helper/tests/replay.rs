//! Offline field replay: feeds a captured raw sensor stream (sub-type + three
//! i16 values per frame, extracted from SDA_HEAD_TRACKING_DEBUG=1 logs) through
//! the production HeadOrientation pipeline and reports per-session wander.
//! Data lives outside the repo; run with SDA_REPLAY_STREAM=<path>.

use std::env;
use std::fs;
use std::path::Path;

mod protocol {
    #[derive(Clone, Copy, Debug)]
    pub struct Orientation {
        pub x: f64,
        pub y: f64,
        pub z: f64,
        pub w: f64,
    }
}

include!("../src/orientation.rs");

const PREFIX: [u8; 10] = [0x04, 0x00, 0x04, 0x00, 0x17, 0x00, 0x00, 0x00, 0x10, 0x00];

fn frame(subtype: u8, o1: i16, o2: i16, o3: i16) -> Vec<u8> {
    let mut packet = vec![0; 72];
    packet[..PREFIX.len()].copy_from_slice(&PREFIX);
    packet[10] = subtype;
    for (offset, value) in [(43, o1), (45, o2), (47, o3)] {
        packet[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    packet
}

fn attitude_degrees(o: &Orientation) -> (f64, f64) {
    let yaw = (o.z.atan2(o.w) * 2.0).to_degrees();
    let pitch = (o.x.atan2(o.w) * 2.0).to_degrees();
    (yaw, pitch)
}

#[test]
fn replay_captured_stream() {
    let path = match env::var("SDA_REPLAY_STREAM") {
        Ok(path) => path,
        Err(_) => {
            eprintln!("skipped (set SDA_REPLAY_STREAM)");
            return;
        }
    };
    assert!(Path::new(&path).exists(), "replay stream missing: {path}");
    let mut tracker = HeadOrientation::default();
    let mut session = 0usize;
    let mut frames = 0usize;
    let mut poses: Vec<(usize, usize, f64, f64)> = Vec::new();
    for line in fs::read_to_string(&path).unwrap().lines() {
        if line == "S" {
            tracker.begin_transport_session();
            session += 1;
            frames = 0;
            continue;
        }
        let mut parts = line.split_whitespace();
        assert_eq!(parts.next(), Some("F"));
        let subtype = u8::from_str_radix(parts.next().unwrap().trim_start_matches("0x"), 16).unwrap();
        let raw: Vec<i16> = parts.map(|p| p.parse().unwrap()).collect();
        let attitude = tracker.process_packet(&frame(subtype, raw[0], raw[1], raw[2]));
        frames += 1;
        if let Some(o) = attitude {
            let (yaw, pitch) = attitude_degrees(&o);
            poses.push((session, frames, yaw, pitch));
        }
    }

    // Per-session wander report plus the worst still-window excursion.
    let mut sessions: Vec<usize> = poses.iter().map(|p| p.0).collect();
    sessions.dedup();
    for session in sessions {
        let rows: Vec<&(usize, usize, f64, f64)> = poses.iter().filter(|p| p.0 == session).collect();
        let (mut yaw_min, mut yaw_max) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut pitch_min, mut pitch_max) = (f64::INFINITY, f64::NEG_INFINITY);
        for row in &rows {
            yaw_min = yaw_min.min(row.2);
            yaw_max = yaw_max.max(row.2);
            pitch_min = pitch_min.min(row.3);
            pitch_max = pitch_max.max(row.3);
        }
        println!(
            "session {session}: {} frames, yaw range {:.2}..{:.2} (span {:.2}), pitch range {:.2}..{:.2} (span {:.2})",
            rows.len(),
            yaw_min, yaw_max, yaw_max - yaw_min,
            pitch_min, pitch_max, pitch_max - pitch_min,
        );

        // Still windows: >= 300 frames where consecutive pose steps are all
        // below 0.5 deg. Report the worst total excursion inside one.
        let mut worst: f64 = 0.0;
        let mut window_start = 0;
        for i in 1..rows.len() {
            let step = ((rows[i].2 - rows[i - 1].2).abs()).max((rows[i].3 - rows[i - 1].3).abs());
            if step >= 0.5 || i == rows.len() - 1 {
                let length = i - window_start;
                if length >= 300 {
                    let seg = &rows[window_start..i];
                    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
                    for row in seg {
                        lo = lo.min(row.2);
                        hi = hi.max(row.2);
                    }
                    worst = worst.max(hi - lo);
                }
                window_start = i;
            }
        }
        println!("session {session}: worst still-window yaw excursion {worst:.2} deg");
    }
}
