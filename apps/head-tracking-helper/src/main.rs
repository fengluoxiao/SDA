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

enum Control {
    Stop,
    Recenter,
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

    loop {
        match poll_control(&controls, &mut orientation, session)? {
            ControlAction::Stop => return Ok(()),
            ControlAction::Continue => {}
        }
        emit_status(session, "disconnected", "正在连接已配对的 AirPods")?;

        let socket = match L2capSocket::connect_airpods() {
            Ok(socket) => socket,
            Err(error) => {
                eprintln!("AirPods transport: {error}");
                emit_status(session, "unavailable", public_connection_error(&error))?;
                if wait_for_retry(&controls, &mut orientation, session)? {
                    return Ok(());
                }
                continue;
            }
        };

        if let Err(error) = initialize_aacp(&socket) {
            eprintln!("AirPods transport: {error}");
            emit_status(session, "disconnected", public_connection_error(&error))?;
            if wait_for_retry(&controls, &mut orientation, session)? {
                return Ok(());
            }
            continue;
        }
        orientation.reset();
        emit_status(
            session,
            "connected",
            "AirPods 已连接，等待 Windows 媒体播放",
        )?;

        let mut packet = [0_u8; 4096];
        let mut received_packets = 0_u64;
        let mut last_motion_at = Instant::now();
        let mut recovery_attempts = 0_u8;
        let mut head_tracking_packet_index = 0_usize;
        let mut audio_source = None;
        let mut motion_was_active = false;
        let disconnected = loop {
            match poll_control(&controls, &mut orientation, session)? {
                ControlAction::Stop => {
                    let _ = stop_motion_stream(&socket, head_tracking_packet_index);
                    return Ok(());
                }
                ControlAction::Continue => {}
            }
            match socket.receive_packet(&mut packet) {
                Ok(Some(length)) => {
                    received_packets = received_packets.saturating_add(1);
                    let data = &packet[..length];
                    if orientation::is_head_tracking_packet(data) {
                        last_motion_at = Instant::now();
                        recovery_attempts = 0;
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
                    if data.len() >= 9 && data[..6] == [0x04, 0x00, 0x04, 0x00, 0x2e, 0x00] {
                        eprintln!("AirPods transport: connected-device count is {}", data[8]);
                        for index in 0..data[8] as usize {
                            let offset = 9 + index * 8;
                            if let Some(device) = data.get(offset..offset + 8) {
                                eprintln!(
                                    "AirPods transport: connected device {} is {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} ({:02x}/{:02x})",
                                    index + 1,
                                    device[0],
                                    device[1],
                                    device[2],
                                    device[3],
                                    device[4],
                                    device[5],
                                    device[6],
                                    device[7]
                                );
                            }
                        }
                    }
                    if let Some(next_audio_source) = parse_audio_source(data) {
                        let was_local_media =
                            is_local_media_source(audio_source, socket.local_address());
                        audio_source = Some(next_audio_source);
                        let is_local_media =
                            is_local_media_source(audio_source, socket.local_address());
                        eprintln!(
                            "AirPods transport: audio source is {:012x} (type={})",
                            next_audio_source.0, next_audio_source.1
                        );
                        if is_local_media && !was_local_media {
                            orientation.reset();
                            motion_was_active = false;
                            recovery_attempts = 0;
                            emit_status(
                                session,
                                "connected",
                                "Windows 媒体已接管 AirPods，正在启动头追",
                            )?;
                            if let Err(error) =
                                recover_motion_stream(&socket, head_tracking_packet_index)
                            {
                                break format!("AirPods motion recovery failed: {error}");
                            }
                            last_motion_at = Instant::now();
                        } else if !is_local_media && was_local_media {
                            orientation.reset();
                            motion_was_active = false;
                            recovery_attempts = 0;
                            emit_status(
                                session,
                                "connected",
                                "AirPods 媒体已切换，等待切回 Windows",
                            )?;
                            last_motion_at = Instant::now();
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
                        emit(&Pose {
                            kind: "pose",
                            protocol: PROTOCOL_VERSION,
                            session,
                            seq: sequence,
                            timestamp_ms: unix_time_ms(),
                            orientation: value,
                        })?;
                    } else if !was_calibrated && orientation.calibrated() {
                        emit_status(session, "connected", "AirPods motion stream active")?;
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
                emit_status(
                    session,
                    "connected",
                    if motion_was_active {
                        "AirPods motion 暂停，正在自动恢复"
                    } else {
                        "Windows 媒体已连接，正在等待 AirPods motion"
                    },
                )?;
                if let Err(error) = recover_motion_stream(&socket, head_tracking_packet_index) {
                    break format!("AirPods motion recovery failed: {error}");
                }
                last_motion_at = Instant::now();
            }
        };
        let _ = stop_motion_stream(&socket, head_tracking_packet_index);
        eprintln!("AirPods transport: {disconnected}");
        emit_status(
            session,
            "disconnected",
            public_connection_error(&disconnected),
        )?;
        if wait_for_retry(&controls, &mut orientation, session)? {
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

fn stop_motion_stream(socket: &L2capSocket, packet_index: usize) -> Result<(), String> {
    socket.send_packet(STOP_HEAD_TRACKING_PACKETS[packet_index])
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

enum ControlAction {
    Continue,
    Stop,
}

fn poll_control(
    controls: &Receiver<Control>,
    orientation: &mut HeadOrientation,
    session: &str,
) -> Result<ControlAction, String> {
    loop {
        match controls.try_recv() {
            Ok(Control::Stop) | Err(TryRecvError::Disconnected) => return Ok(ControlAction::Stop),
            Ok(Control::Recenter) => {
                orientation.reset();
                emit_status(session, "connected", "正在重新校准，请保持面向前方")?;
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
) -> Result<bool, String> {
    for _ in 0..15 {
        if matches!(
            poll_control(controls, orientation, session)?,
            ControlAction::Stop
        ) {
            return Ok(true);
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
    }
}
