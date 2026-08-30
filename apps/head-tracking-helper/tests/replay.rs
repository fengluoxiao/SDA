//! Offline field replay: feeds a captured raw sensor stream (sub-type + three
//! i16 values per frame, extracted from SDA_HEAD_TRACKING_DEBUG=1 logs) through
//! the production HeadOrientation pipeline and measures the output excursion
//! inside device-still windows. Data lives outside the repo; run with
//! SDA_REPLAY_STREAM=<path>.

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

fn attitude_angle(from: &Orientation, to: &Orientation) -> f64 {
    let dot = (from.x * to.x + from.y * to.y + from.z * to.z + from.w * to.w).abs().min(1.0);
    2.0 * dot.acos().to_degrees()
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

    // raw_log[session] = [(local_frame, v2, v3)]
    let mut raw_log: Vec<Vec<(usize, i16, i16)>> = vec![Vec::new()];
    let mut poses: Vec<Vec<(usize, Orientation)>> = vec![Vec::new()];
    let mut tracker = HeadOrientation::default();
    for line in fs::read_to_string(&path).unwrap().lines() {
        if line == "S" {
            tracker.begin_transport_session();
            raw_log.push(Vec::new());
            poses.push(Vec::new());
            continue;
        }
        let mut parts = line.split_whitespace();
        assert_eq!(parts.next(), Some("F"));
        let subtype = u8::from_str_radix(parts.next().unwrap().trim_start_matches("0x"), 16).unwrap();
        let raw: Vec<i16> = parts.map(|p| p.parse().unwrap()).collect();
        let session = raw_log.len() - 1;
        let local = raw_log[session].len();
        raw_log[session].push((local, raw[1], raw[2]));
        if let Some(o) = tracker.process_packet(&frame(subtype, raw[0], raw[1], raw[2])) {
            poses[session].push((local, o));
        }
    }

    for (session, raws) in raw_log.iter().enumerate() {
        if raws.len() < 10 {
            continue;
        }
        // Device-still windows: |Δv2| and |Δv3| < 30 counts for >= 900 frames.
        let mut windows: Vec<(usize, usize)> = Vec::new();
        let mut start = 0usize;
        for i in 1..raws.len() {
            let still = (raws[i].1 as i32 - raws[i - 1].1 as i32).abs() < 30
                && (raws[i].2 as i32 - raws[i - 1].2 as i32).abs() < 30;
            if !still {
                if i - start >= 900 {
                    windows.push((raws[start].0, raws[i - 1].0));
                }
                start = i;
            }
        }
        let mut worst: f64 = 0.0;
        let session_poses = &poses[session];
        for (begin, end) in &windows {
            let begin_pose = session_poses.iter().position(|(local, _)| local >= begin);
            let end_pose = session_poses.iter().rposition(|(local, _)| local <= end);
            if let (Some(b), Some(e)) = (begin_pose, end_pose) {
                if e > b {
                    worst = worst.max(attitude_angle(&session_poses[b].1, &session_poses[e].1));
                }
            }
        }
        println!(
            "session {session}: {} frames, {} poses, {} still windows (>=30s), worst still excursion {:.2} deg",
            raws.len(),
            session_poses.len(),
            windows.len(),
            worst,
        );
    }
}
