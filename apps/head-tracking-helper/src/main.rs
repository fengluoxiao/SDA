mod orientation;
mod protocol;
mod windows_transport;

use std::io::{self, BufRead, BufReader, Write};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use orientation::HeadOrientation;
use protocol::{Command, CommandType, ErrorMessage, Hello, PROTOCOL_VERSION, Pose, Status};
use serde::Serialize;
use windows_transport::L2capSocket;

const HANDSHAKE: &[u8] = &[
    0x00, 0x00, 0x04, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
const SET_SPECIFIC_FEATURES: &[u8] = &[
    0x04, 0x00, 0x04, 0x00, 0x4d, 0x00, 0xd7, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
const REQUEST_NOTIFICATIONS: &[u8] = &[0x04, 0x00, 0x04, 0x00, 0x0f, 0x00, 0xff, 0xff, 0xff, 0xff];
const CLAIM_OWNERSHIP: &[u8] = &[
    0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x06, 0x01, 0x00, 0x00, 0x00,
];
const RELEASE_OWNERSHIP: &[u8] = &[
    0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x06, 0x00, 0x00, 0x00, 0x00,
];
const START_HEAD_TRACKING_ALTERNATE: &[u8] = &[
    0x04, 0x00, 0x04, 0x00, 0x17, 0x00, 0x00, 0x00, 0x10, 0x00, 0x0f, 0x00, 0x08, 0x73, 0x42, 0x0b,
    0x08, 0x10, 0x10, 0x02, 0x1a, 0x05, 0x01, 0x40, 0x9c, 0x00, 0x00,
];
const START_HEAD_TRACKING_STANDARD: &[u8] = &[
    0x04, 0x00, 0x04, 0x00, 0x17, 0x00, 0x00, 0x00, 0x10, 0x00, 0x10, 0x00, 0x08, 0xa1, 0x02, 0x42,
    0x0b, 0x08, 0x0e, 0x10, 0x02, 0x1a, 0x05, 0x01, 0x40, 0x9c, 0x00, 0x00,
];
const STOP_HEAD_TRACKING_ALTERNATE: &[u8] = &[
    0x04, 0x00, 0x04, 0x00, 0x17, 0x00, 0x00, 0x00, 0x10, 0x00, 0x0f, 0x00, 0x08, 0x75, 0x42, 0x0b,
    0x08, 0x10, 0x10, 0x02, 0x1a, 0x05, 0x01, 0x00, 0x00, 0x00, 0x00,
];
const STOP_HEAD_TRACKING_STANDARD: &[u8] = &[
    0x04, 0x00, 0x04, 0x00, 0x17, 0x00, 0x00, 0x00, 0x10, 0x00, 0x11, 0x00, 0x08, 0x7e, 0x10, 0x02,
    0x42, 0x0b, 0x08, 0x4e, 0x10, 0x02, 0x1a, 0x05, 0x01, 0x00, 0x00, 0x00, 0x00,
];
const START_HEAD_TRACKING_PACKETS: [&[u8]; 2] =
    [START_HEAD_TRACKING_ALTERNATE, START_HEAD_TRACKING_STANDARD];
const STOP_HEAD_TRACKING_PACKETS: [&[u8]; 2] =
    [STOP_HEAD_TRACKING_ALTERNATE, STOP_HEAD_TRACKING_STANDARD];
const MOTION_IDLE_TIMEOUT: Duration = Duration::from_secs(2);
const MOTION_RECOVERY_ATTEMPTS: u8 = 3;
const TAKEOVER_SOURCE_GRACE: Duration = Duration::from_secs(10);
const LOCAL_AUDIO_SOURCE_TYPE: u8 = 2;

enum Control {
    Stop,
    Recenter,
    Takeover,
    Fatal(String),
}

fn main() {
    if let Err(error) = run() {
        eprintln!("SDA AirPods head-tracking helper: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let stdin = io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let first_line = lines
        .next()
        .ok_or_else(|| "SDA did not send a start command".to_string())?
        .map_err(|_| "could not read SDA start command")?;
    let start = Command::parse(&first_line)?;
    if start.kind != CommandType::Start {
        return Err("the first command must be start".into());
    }
    let session = start.session;

    emit(&Hello {
        kind: "hello",
        protocol: PROTOCOL_VERSION,
        session: &session,
        source: "windows-airpods-experimental",
        coordinate_system: "sda-adm-right-forward-up",
        orientation: "head-to-world-quaternion",
    })?;

    let (control_tx, control_rx) = mpsc::channel();
    let command_session = session.clone();
    thread::spawn(move || {
        for line in lines {
            let control = match line {
                Ok(line) => match Command::parse(&line).and_then(|command| {
                    command.validate_session(&command_session)?;
                    Ok(command.kind)
                }) {
                    Ok(CommandType::Stop) => Control::Stop,
                    Ok(CommandType::Recenter) => Control::Recenter,
                    Ok(CommandType::Takeover) => Control::Takeover,
                    Ok(CommandType::Start) => continue,
                    Err(error) => Control::Fatal(error),
                },
                Err(_) => Control::Fatal("could not read SDA command".into()),
            };
            let should_stop = matches!(control, Control::Stop | Control::Fatal(_));
            if control_tx.send(control).is_err() || should_stop {
                return;
            }
        }
        let _ = control_tx.send(Control::Stop);
    });

    tracking_loop(&session, control_rx)
}

fn tracking_loop(session: &str, controls: Receiver<Control>) -> Result<(), String> {
    let mut orientation = HeadOrientation::default();
    let mut sequence = 0_u64;
    let mut takeover_pending = false;
    // SDA_HEAD_TRACKING_DEBUG=1: per-frame raw sensor state on stderr,
    // forwarded to the Electron console by the desktop shell in dev.
    let debug_frames = std::env::var("SDA_HEAD_TRACKING_DEBUG").map(|v| v == "1").unwrap_or(false);

    loop {
        match poll_control(&controls, &mut orientation, session)? {
            ControlAction::Stop => return Ok(()),
            ControlAction::Takeover => takeover_pending = true,
            ControlAction::Continue => {}
        }
        emit_status(session, "disconnected", "正在连接已配对的 AirPods")?;

        let socket = match L2capSocket::connect_airpods() {
            Ok(socket) => socket,
            Err(error) => {
                eprintln!("AirPods transport: {error}");
                emit_status(session, "unavailable", public_connection_error(&error))?;
                if wait_for_retry(&controls, &mut orientation, session, &mut takeover_pending)? {
                    return Ok(());
                }
                continue;
            }
        };

        if let Err(error) = initialize_aacp(&socket) {
            eprintln!("AirPods transport: {error}");
            emit_status(session, "disconnected", public_connection_error(&error))?;
            if wait_for_retry(&controls, &mut orientation, session, &mut takeover_pending)? {
                return Ok(());
            }
            continue;
        }
        let mut head_tracking_packet_index = 0_usize;
        if let Err(error) = recover_motion_stream(&socket, head_tracking_packet_index) {
            eprintln!("AirPods transport: initial motion claim failed: {error}");
            emit_status(session, "disconnected", public_connection_error(&error))?;
            if wait_for_retry(&controls, &mut orientation, session, &mut takeover_pending)? {
                return Ok(());
            }
            continue;
        }
        orientation.begin_transport_session();
        emit_status(
            session,
            "connected",
            "Windows 媒体已连接，正在启动 AirPods motion",
        )?;

        let mut packet = [0_u8; 4096];
        let mut received_packets = 0_u64;
        let mut last_motion_at = Instant::now();
        let mut recovery_attempts = 0_u8;
        // AirPods only reports audio-source changes, not necessarily the current
        // source when AACP reconnects. Treat the local endpoint as current until
        // a real notification says otherwise so an Electron restart can reclaim
        // motion without requiring an audio handoff first.
        let mut audio_source = Some(inferred_local_audio_source(socket.local_address()));
        let mut local_media_confirmed = false;
        let mut connected_devices = Vec::new();
        let mut force_takeover_attempted = false;
        let mut takeover_grace_until = None;
        let mut motion_was_active = false;
        let disconnected = loop {
            match poll_control(&controls, &mut orientation, session)? {
                ControlAction::Stop => {
                    let _ = stop_motion_stream(&socket);
                    return Ok(());
                }
                ControlAction::Takeover => {
                    takeover_pending = true;
                    emit_status(
                        session,
                        "connected",
                        "Windows 正在强制接管整个 AirPods 连接",
                    )?;
                }
                ControlAction::Continue => {}
            }
            if takeover_pending && !connected_devices.is_empty() {
                if let Err(error) = force_reclaim_entire_connection(
                    &socket,
                    head_tracking_packet_index,
                    &connected_devices,
                ) {
                    break format!("AirPods full connection takeover failed: {error}");
                }
                takeover_pending = false;
                emit_status(
                    session,
                    "connected",
                    "Windows 已接管 AirPods，正在启动 motion",
                )?;
                takeover_grace_until = Some(Instant::now() + TAKEOVER_SOURCE_GRACE);
                audio_source = Some(inferred_local_audio_source(socket.local_address()));
                local_media_confirmed = true;
                force_takeover_attempted = true;
                motion_was_active = false;
                recovery_attempts = 0;
                last_motion_at = Instant::now();
            }
            match socket.receive_packet(&mut packet) {
                Ok(Some(length)) => {
                    received_packets = received_packets.saturating_add(1);
                    let data = &packet[..length];
                    if orientation::is_head_tracking_packet(data) {
                        last_motion_at = Instant::now();
                        recovery_attempts = 0;
                        force_takeover_attempted = false;
                        motion_was_active = true;
                    }
                    if data.len() >= 8 && data[..7] == [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x06] {
                        eprintln!("AirPods transport: ownership state is {}", data[7] == 0x01);
                    }
                    if data.len() >= 8 && data[..6] == [0x04, 0x00, 0x04, 0x00, 0x09, 0x00] {
                        eprintln!(
                            "AirPods transport: control id=0x{:02x}, state=0x{:02x}",
                            data[6], data[7]
                        );
                    }
                    if let Some(next_connected_devices) = parse_connected_devices(data) {
                        connected_devices = next_connected_devices;
                        eprintln!(
                            "AirPods transport: connected-device count is {}",
                            connected_devices.len()
                        );
                        for (index, device) in connected_devices.iter().enumerate() {
                            eprintln!(
                                "AirPods transport: connected device {} is {}",
                                index + 1,
                                format_bluetooth_address(*device)
                            );
                        }
                    }
                    if let Some(next_audio_source) = parse_audio_source(data) {
                        let was_local_media_confirmed = local_media_confirmed;
                        let was_local_media =
                            is_local_media_source(audio_source, socket.local_address());
                        audio_source = Some(next_audio_source);
                        let is_local_media =
                            is_local_media_source(audio_source, socket.local_address());
                        let is_remote_media =
                            is_remote_media_source(audio_source, socket.local_address());
                        let ignore_stale_remote = is_remote_media
                            && takeover_grace_until.is_some_and(|until| Instant::now() < until);
                        if !ignore_stale_remote {
                            local_media_confirmed = is_local_media;
                        }
                        eprintln!(
                            "AirPods transport: audio source is {:012x} (type={})",
                            next_audio_source.0, next_audio_source.1
                        );
                        if ignore_stale_remote {
                            audio_source =
                                Some(inferred_local_audio_source(socket.local_address()));
                            eprintln!(
                                "AirPods transport: ignoring stale remote source during Windows takeover"
                            );
                        } else if is_local_media && !was_local_media_confirmed {
                            motion_was_active = false;
                            recovery_attempts = 0;
                            emit_status(
                                session,
                                "connected",
                                "Windows 媒体已接管 AirPods，正在启动头追",
                            )?;
                            if let Err(error) = force_reclaim_entire_connection(
                                &socket,
                                head_tracking_packet_index,
                                &connected_devices,
                            ) {
                                break format!("AirPods motion recovery failed: {error}");
                            }
                            force_takeover_attempted = true;
                            takeover_grace_until = Some(Instant::now() + TAKEOVER_SOURCE_GRACE);
                            last_motion_at = Instant::now();
                        } else if is_remote_media {
                            motion_was_active = false;
                            recovery_attempts = 0;
                            force_takeover_attempted = false;
                            emit_status(
                                session,
                                "connected",
                                "AirPods 媒体已切换，等待切回 Windows",
                            )?;
                            last_motion_at = Instant::now();
                        } else if was_local_media {
                            force_takeover_attempted = false;
                            emit_status(
                                session,
                                "connected",
                                "AirPods 已连接，等待 Windows 媒体播放",
                            )?;
                        }
                    }
                    if data.len() >= 8 && data[..6] == [0x04, 0x00, 0x04, 0x00, 0x06, 0x00] {
                        let ear_state = |value| match value {
                            0x00 => "in-ear",
                            0x01 => "out-of-ear",
                            0x02 => "in-case",
                            0x03 => "disconnected",
                            _ => "unknown",
                        };
                        eprintln!(
                            "AirPods transport: ear state is {}/{}",
                            ear_state(data[6]),
                            ear_state(data[7])
                        );
                    }
                    if received_packets <= 12 {
                        let contains_head_tracking = data
                            .windows(6)
                            .any(|value| value == [0x04, 0x00, 0x04, 0x00, 0x17, 0x00]);
                        let ownership = data.windows(8).find_map(|value| {
                            (value[..7] == [0x04, 0x00, 0x04, 0x00, 0x09, 0x00, 0x06])
                                .then_some(value[7] == 0x01)
                        });
                        let category = if data.starts_with(&[0x01, 0x00, 0x04, 0x00]) {
                            "handshake-ack"
                        } else if data.starts_with(&[0x04, 0x00, 0x04, 0x00]) {
                            match data.get(4) {
                                Some(0x04) => "battery",
                                Some(0x06) => "ear-detection",
                                Some(0x09) => "control-command",
                                Some(0x0e) => "audio-source",
                                Some(0x17) => "head-tracking",
                                Some(0x1d) => "device-information",
                                Some(0x2b) => "feature-ack",
                                Some(0x2e) => "connected-devices",
                                Some(0x4b) => "conversation-awareness",
                                _ => "aacp-control",
                            }
                        } else {
                            "other"
                        };
                        let opcode = data.get(4).copied().unwrap_or_default();
                        eprintln!(
                            "AirPods transport: received {category} packet (opcode=0x{opcode:02x}, {length} bytes, embedded-motion={contains_head_tracking}, ownership={ownership:?})"
                        );
                    }
                    let was_calibrated = orientation.calibrated();
                    if let Some(value) = orientation.process_packet(&packet[..length]) {
                        sequence = sequence.saturating_add(1);
                        if debug_frames {
                            eprintln!("{}", orientation.debug_state());
                        }
                        emit(&Pose {
                            kind: "pose",
                            protocol: PROTOCOL_VERSION,
                            session,
                            seq: sequence,
                            timestamp_ms: unix_time_ms(),
                            orientation: value,
                        })?;
                    } else if !was_calibrated && orientation.calibrated() {
                        emit_status(
                            session,
                            "connected",
                            "AirPods motion stream active (fixed-PC drift lock)",
                        )?;
                    }
                }
                Ok(None) => {}
                Err(error) => break error,
            }
            if motion_stream_timed_out(last_motion_at, Instant::now()) {
                if !is_local_media_source(audio_source, socket.local_address()) {
                    last_motion_at = Instant::now();
                    continue;
                }
                if motion_was_active && recovery_attempts >= MOTION_RECOVERY_ATTEMPTS {
                    break "AirPods motion stream stalled after recovery".into();
                }
                recovery_attempts = recovery_attempts.saturating_add(1);
                head_tracking_packet_index =
                    (head_tracking_packet_index + 1) % START_HEAD_TRACKING_PACKETS.len();
                eprintln!(
                    "AirPods transport: motion idle on local media; soft recovery {recovery_attempts} (head-tracking packet {})",
                    head_tracking_packet_index + 1
                );
                let force_reclaim = local_media_confirmed && !force_takeover_attempted;
                emit_status(
                    session,
                    "connected",
                    if force_reclaim {
                        "Windows 正在强制接管 AirPods motion"
                    } else if motion_was_active {
                        "AirPods motion 暂停，正在自动恢复"
                    } else {
                        "Windows 媒体已连接，正在等待 AirPods motion"
                    },
                )?;
                let recovery = if force_reclaim {
                    force_reclaim_entire_connection(
                        &socket,
                        head_tracking_packet_index,
                        &connected_devices,
                    )
                } else {
                    recover_motion_stream(&socket, head_tracking_packet_index)
                };
                if let Err(error) = recovery {
                    break format!("AirPods motion recovery failed: {error}");
                }
                force_takeover_attempted |= force_reclaim;
                last_motion_at = Instant::now();
            }
        };
        let _ = stop_motion_stream(&socket);
        eprintln!("AirPods transport: {disconnected}");
        emit_status(
            session,
            "disconnected",
            public_connection_error(&disconnected),
        )?;
        if wait_for_retry(&controls, &mut orientation, session, &mut takeover_pending)? {
            return Ok(());
        }
    }
}

fn initialize_aacp(socket: &L2capSocket) -> Result<(), String> {
    socket.send_packet(HANDSHAKE)?;
    thread::sleep(Duration::from_millis(300));
    socket.send_packet(SET_SPECIFIC_FEATURES)?;
    thread::sleep(Duration::from_millis(300));
    socket.send_packet(REQUEST_NOTIFICATIONS)
}

fn recover_motion_stream(socket: &L2capSocket, packet_index: usize) -> Result<(), String> {
    // Head-tracking state is global to the buds and can survive an audio handoff.
    // Clear either LibrePods packet variant before claiming the new local session.
    for packet in STOP_HEAD_TRACKING_PACKETS {
        socket.send_packet(packet)?;
        thread::sleep(Duration::from_millis(50));
    }
    socket.send_packet(CLAIM_OWNERSHIP)?;
    thread::sleep(Duration::from_millis(150));
    socket.send_packet(START_HEAD_TRACKING_PACKETS[packet_index])
}

fn force_reclaim_entire_connection(
    socket: &L2capSocket,
    packet_index: usize,
    connected_devices: &[u64],
) -> Result<(), String> {
    stop_motion_stream(socket)?;
    socket.send_packet(RELEASE_OWNERSHIP)?;
    thread::sleep(Duration::from_millis(100));
    socket.send_packet(CLAIM_OWNERSHIP)?;

    for address in connected_devices {
        if *address == socket.local_address() {
            continue;
        }
        socket.send_packet(&smart_routing_media_information_packet(
            socket.local_address(),
            *address,
        ))?;
        thread::sleep(Duration::from_millis(50));
        socket.send_packet(&smart_routing_show_ui_packet(*address))?;
        thread::sleep(Duration::from_millis(50));
        socket.send_packet(&smart_routing_hijack_packet(*address))?;
        thread::sleep(Duration::from_millis(50));
    }

    thread::sleep(Duration::from_millis(500));
    socket.send_packet(REQUEST_NOTIFICATIONS)?;
    socket.send_packet(CLAIM_OWNERSHIP)?;
    thread::sleep(Duration::from_millis(150));
    socket.send_packet(START_HEAD_TRACKING_PACKETS[packet_index])
}

fn stop_motion_stream(socket: &L2capSocket) -> Result<(), String> {
    for packet in STOP_HEAD_TRACKING_PACKETS {
        socket.send_packet(packet)?;
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

fn inferred_local_audio_source(local_address: u64) -> (u64, u8) {
    (local_address, LOCAL_AUDIO_SOURCE_TYPE)
}

fn parse_connected_devices(packet: &[u8]) -> Option<Vec<u64>> {
    let start = packet
        .windows(9)
        .position(|frame| frame[..6] == [0x04, 0x00, 0x04, 0x00, 0x2e, 0x00])?;
    let count = packet[start + 8] as usize;
    let devices = packet.get(start + 9..start + 9 + count * 8)?;
    Some(
        devices
            .chunks_exact(8)
            .map(|device| {
                device[..6]
                    .iter()
                    .fold(0_u64, |address, byte| (address << 8) | *byte as u64)
            })
            .collect(),
    )
}

fn smart_routing_hijack_packet(target_address: u64) -> Vec<u8> {
    let mut packet = vec![0x04, 0x00, 0x04, 0x00, 0x10, 0x00];
    packet.extend_from_slice(&target_address.to_le_bytes()[..6]);
    packet.extend_from_slice(&[0x62, 0x00, 0x01, 0xe5, 0x4a]);
    packet.extend_from_slice(b"localscore");
    packet.extend_from_slice(&[0x30, 0x64, 0x46]);
    packet.extend_from_slice(b"reason");
    packet.push(0x48);
    packet.extend_from_slice(b"Hijackv2");
    packet.push(0x51);
    packet.extend_from_slice(b"audioRoutingScore");
    packet.extend_from_slice(&[0x31, 0x2d, 0x01, 0x5f]);
    packet.extend_from_slice(b"audioRoutingSetOwnershipToFalse");
    packet.extend_from_slice(&[0x01, 0x4b]);
    packet.extend_from_slice(b"remotescore");
    packet.push(0xa5);
    packet.resize(112, 0);
    packet
}

fn smart_routing_media_information_packet(local_address: u64, target_address: u64) -> Vec<u8> {
    let mut packet = vec![0x04, 0x00, 0x04, 0x00, 0x10, 0x00];
    packet.extend_from_slice(&target_address.to_le_bytes()[..6]);
    packet.extend_from_slice(&[0x82, 0x00, 0x01, 0xe5, 0x4a]);
    packet.extend_from_slice(b"PlayingApp");
    packet.push(0x56);
    packet.extend_from_slice(b"com.google.ios.youtube");
    packet.push(0x52);
    packet.extend_from_slice(b"HostStreamingState");
    packet.extend_from_slice(&[0x42, b'Y', b'E', b'S', 0x49]);
    packet.extend_from_slice(b"btAddress");
    packet.push(0x51);
    packet.extend_from_slice(format_bluetooth_address(local_address).as_bytes());
    packet.extend_from_slice(b"btName");
    packet.push(0x47);
    packet.extend_from_slice(b"Windows");
    packet.push(0x58);
    packet.extend_from_slice(b"otherDevice");
    packet.extend_from_slice(b"AudioCategory");
    packet.extend_from_slice(&[0x31, 0x2d, 0x01]);
    packet.resize(144, 0);
    packet
}

fn smart_routing_show_ui_packet(target_address: u64) -> Vec<u8> {
    let mut packet = vec![0x04, 0x00, 0x04, 0x00, 0x10, 0x00];
    packet.extend_from_slice(&target_address.to_le_bytes()[..6]);
    packet.extend_from_slice(&[0x7e, 0x00, 0x01, 0xe6, 0x5b]);
    packet.extend_from_slice(b"SmartRoutingKeyShowNearbyUI");
    packet.extend_from_slice(&[0x01, 0x4a]);
    packet.extend_from_slice(b"localscore");
    packet.extend_from_slice(&[0x31, 0x2d, 0x01, 0x46]);
    packet.extend_from_slice(b"reasonHhijackv2");
    packet.push(0x51);
    packet.extend_from_slice(b"audioRoutingScore");
    packet.extend_from_slice(&[0xa2, 0x5f]);
    packet.extend_from_slice(b"audioRoutingSetOwnershipToFalse");
    packet.extend_from_slice(&[0x01, 0x4b]);
    packet.extend_from_slice(b"remotescore");
    packet.push(0xa2);
    packet.resize(140, 0);
    packet
}

fn format_bluetooth_address(address: u64) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        (address >> 40) & 0xff,
        (address >> 32) & 0xff,
        (address >> 24) & 0xff,
        (address >> 16) & 0xff,
        (address >> 8) & 0xff,
        address & 0xff
    )
}

fn parse_audio_source(packet: &[u8]) -> Option<(u64, u8)> {
    packet.windows(13).find_map(|frame| {
        if frame[..6] != [0x04, 0x00, 0x04, 0x00, 0x0e, 0x00] {
            return None;
        }
        let address = frame[6..12]
            .iter()
            .enumerate()
            .fold(0_u64, |value, (shift, byte)| {
                value | ((*byte as u64) << (shift * 8))
            });
        Some((address, frame[12]))
    })
}

fn is_local_media_source(source: Option<(u64, u8)>, local_address: u64) -> bool {
    matches!(source, Some((address, 0x02)) if address == local_address)
}

fn is_remote_media_source(source: Option<(u64, u8)>, local_address: u64) -> bool {
    matches!(source, Some((address, 0x01 | 0x02)) if address != local_address)
}

enum ControlAction {
    Continue,
    Stop,
    Takeover,
}

fn poll_control(
    controls: &Receiver<Control>,
    _orientation: &mut HeadOrientation,
    session: &str,
) -> Result<ControlAction, String> {
    loop {
        match controls.try_recv() {
            Ok(Control::Stop) | Err(TryRecvError::Disconnected) => return Ok(ControlAction::Stop),
            Ok(Control::Takeover) => return Ok(ControlAction::Takeover),
            Ok(Control::Recenter) => {
                emit_status(session, "connected", "当前朝向已设为正前方")?;
            }
            Ok(Control::Fatal(message)) => {
                emit(&ErrorMessage {
                    kind: "error",
                    protocol: PROTOCOL_VERSION,
                    session,
                    code: "invalid-command",
                    message: &message,
                })?;
                return Err(message);
            }
            Err(TryRecvError::Empty) => return Ok(ControlAction::Continue),
        }
    }
}

fn wait_for_retry(
    controls: &Receiver<Control>,
    orientation: &mut HeadOrientation,
    session: &str,
    takeover_pending: &mut bool,
) -> Result<bool, String> {
    for _ in 0..15 {
        match poll_control(controls, orientation, session)? {
            ControlAction::Stop => return Ok(true),
            ControlAction::Takeover => *takeover_pending = true,
            ControlAction::Continue => {}
        }
        thread::sleep(Duration::from_millis(200));
    }
    Ok(false)
}

fn public_connection_error(error: &str) -> &str {
    if error.contains("no paired AirPods") {
        "未找到已配对的 AirPods，请先在 Windows 设置中配对"
    } else if error.contains("no paired Bluetooth") {
        "未找到已配对的蓝牙设备"
    } else if error.contains("L2CAP provider") {
        "此 Windows 蓝牙栈未提供 L2CAP"
    } else if error.contains("(10050)") {
        "AirPods 音频已连接，但 Windows 不允许普通应用打开私有 L2CAP motion 通道（10050）"
    } else if error.contains("AACP connection") {
        "无法连接 AirPods motion 通道；音频连接不代表 motion 通道可用"
    } else if error.contains("disconnected") {
        "AirPods 已断开，正在重连"
    } else if error.contains("motion stream stalled") {
        "AirPods motion 流已中断，等待 Windows 重新连接"
    } else {
        "AirPods motion 通道暂不可用，正在重试"
    }
}

fn motion_stream_timed_out(last_motion_at: Instant, now: Instant) -> bool {
    now.saturating_duration_since(last_motion_at) >= MOTION_IDLE_TIMEOUT
}

fn emit_status(session: &str, state: &'static str, detail: &str) -> Result<(), String> {
    emit(&Status {
        kind: "status",
        protocol: PROTOCOL_VERSION,
        session,
        state,
        detail,
    })
}

fn emit<T: Serialize>(message: &T) -> Result<(), String> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer(&mut output, message).map_err(|_| "could not encode JSON")?;
    output
        .write_all(b"\n")
        .and_then(|_| output.flush())
        .map_err(|_| "SDA output channel closed".into())
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_watchdog_reopens_a_stalled_channel() {
        let started = Instant::now();
        assert!(!motion_stream_timed_out(
            started,
            started + MOTION_IDLE_TIMEOUT - Duration::from_millis(1)
        ));
        assert!(motion_stream_timed_out(
            started,
            started + MOTION_IDLE_TIMEOUT
        ));
    }

    #[test]
    fn cycles_both_librepods_head_tracking_packet_variants() {
        assert_eq!(START_HEAD_TRACKING_PACKETS.len(), 2);
        assert_eq!(STOP_HEAD_TRACKING_PACKETS.len(), 2);
        assert_ne!(
            START_HEAD_TRACKING_PACKETS[0],
            START_HEAD_TRACKING_PACKETS[1]
        );
        assert_ne!(STOP_HEAD_TRACKING_PACKETS[0], STOP_HEAD_TRACKING_PACKETS[1]);
    }

    #[test]
    fn parses_coalesced_audio_source_and_matches_only_local_media() {
        let mut packet = vec![0xaa, 0xbb];
        packet.extend([
            0x04, 0x00, 0x04, 0x00, 0x0e, 0x00, 0xc3, 0x6e, 0x3b, 0x37, 0x5e, 0xe4, 0x02,
        ]);
        let source = parse_audio_source(&packet);
        assert_eq!(source, Some((0xe45e_373b_6ec3, 0x02)));
        assert!(is_local_media_source(source, 0xe45e_373b_6ec3));
        assert!(!is_local_media_source(source, 0x90ec_ea16_dcee));
        assert!(!is_local_media_source(
            Some((0xe45e_373b_6ec3, 0x00)),
            0xe45e_373b_6ec3
        ));
        assert!(!is_remote_media_source(
            Some((0xe45e_373b_6ec3, 0x00)),
            0xe45e_373b_6ec3
        ));
        assert!(is_remote_media_source(
            Some((0x90ec_ea16_dcee, 0x02)),
            0xe45e_373b_6ec3
        ));
    }

    #[test]
    fn reconnect_bootstraps_motion_without_an_audio_source_notification() {
        let local_address = 0xe45e_373b_6ec3;
        let mut source = Some(inferred_local_audio_source(local_address));
        assert!(is_local_media_source(source, local_address));

        source = Some((0x90ec_ea16_dcee, LOCAL_AUDIO_SOURCE_TYPE));
        assert!(!is_local_media_source(source, local_address));
    }

    #[test]
    fn parses_connected_devices_for_smart_routing_takeover() {
        let mut packet = vec![0xaa, 0xbb];
        packet.extend([
            0x04, 0x00, 0x04, 0x00, 0x2e, 0x00, 0x00, 0x00, 0x03, 0x90, 0xec, 0xea, 0x16, 0xdc,
            0xee, 0x00, 0x05, 0x54, 0x62, 0xe2, 0xbe, 0xe4, 0x05, 0x00, 0x01, 0xe4, 0x5e, 0x37,
            0x3b, 0x6e, 0xc3, 0x02, 0x02,
        ]);
        assert_eq!(
            parse_connected_devices(&packet),
            Some(vec![0x90ec_ea16_dcee, 0x5462_e2be_e405, 0xe45e_373b_6ec3])
        );
        assert_eq!(
            format_bluetooth_address(0x90ec_ea16_dcee),
            "90:ec:ea:16:dc:ee"
        );
    }

    #[test]
    fn builds_librepods_hijack_v2_packet_for_remote_device() {
        let packet = smart_routing_hijack_packet(0x90ec_ea16_dcee);
        assert_eq!(packet.len(), 112);
        assert_eq!(&packet[..6], &[0x04, 0x00, 0x04, 0x00, 0x10, 0x00]);
        assert_eq!(&packet[6..12], &[0xee, 0xdc, 0x16, 0xea, 0xec, 0x90]);
        assert!(packet.windows(8).any(|value| value == b"Hijackv2"));
        assert!(
            packet
                .windows(31)
                .any(|value| value == b"audioRoutingSetOwnershipToFalse")
        );

        let media = smart_routing_media_information_packet(0xe45e_373b_6ec3, 0x90ec_ea16_dcee);
        assert_eq!(media.len(), 144);
        assert_eq!(&media[6..12], &[0xee, 0xdc, 0x16, 0xea, 0xec, 0x90]);
        assert!(media.windows(17).any(|value| value == b"e4:5e:37:3b:6e:c3"));
        assert!(media.windows(3).any(|value| value == b"YES"));

        let show_ui = smart_routing_show_ui_packet(0x90ec_ea16_dcee);
        assert_eq!(show_ui.len(), 140);
        assert!(
            show_ui
                .windows(27)
                .any(|value| value == b"SmartRoutingKeyShowNearbyUI")
        );
    }
}
