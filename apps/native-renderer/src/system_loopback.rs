//! Post-render 48 kHz stereo return for the Windows virtual endpoint.
//! Audio callbacks only copy into a bounded SPSC queue; no socket/driver IO there.
use crate::stereo_fifo::StereoFifo;
use std::{
    io::Write,
    net::{SocketAddr, TcpStream},
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

static FIFO: OnceLock<StereoFifo> = OnceLock::new();
static CONTROL: OnceLock<mpsc::Sender<Option<(SocketAddr, String)>>> = OnceLock::new();
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
static READY: AtomicUsize = AtomicUsize::new(0);

pub fn configure(address: Option<String>, token: Option<String>) -> bool {
    let target = match (address, token) {
        (None, None) => None,
        (Some(a), Some(t)) => {
            let Ok(a) = a.parse::<SocketAddr>() else {
                return false;
            };
            if !a.ip().is_loopback() || t.len() != 64 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
                return false;
            }
            Some((a, t))
        }
        _ => return false,
    };
    let tx = CONTROL.get_or_init(|| {
        FIFO.get_or_init(|| StereoFifo::new(4800));
        let (tx, rx) = mpsc::channel::<Option<(SocketAddr, String)>>();
        std::thread::Builder::new()
            .name("sda-system-return".into())
            .spawn(move || {
                let fifo = FIFO.get().unwrap();
                let mut socket: Option<TcpStream> = None;
                let mut generation = 0usize;
                let mut packet = [0u8; 4 + 480 * 8];
                loop {
                    match rx.recv_timeout(Duration::from_millis(2)) {
                        Ok(target) => {
                            ACTIVE.store(0, Ordering::Release);
                            socket = None;
                            if let Some((address, token)) = target {
                                let connected = (|| -> std::io::Result<TcpStream> {
                                    let mut s = TcpStream::connect_timeout(
                                        &address,
                                        Duration::from_millis(250),
                                    )?;
                                    s.set_write_timeout(Some(Duration::from_millis(50)))?;
                                    s.set_nodelay(true)?;
                                    s.write_all(token.as_bytes())?;
                                    s.write_all(b"\n")?;
                                    Ok(s)
                                })();
                                if let Ok(s) = connected {
                                    generation = generation.wrapping_add(1).max(1);
                                    socket = Some(s);
                                    ACTIVE.store(generation, Ordering::Release);
                                }
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    fifo.apply_flush_from_consumer();
                    if socket.is_none() || READY.load(Ordering::Acquire) != generation {
                        continue;
                    }
                    // Never replay a long backlog after a stalled broker.
                    if fifo.available_read() > 2400 {
                        fifo.pop_frames(fifo.available_read(), |_, _| {});
                    }
                    let n = fifo.pop_frames(480, |i, frame| {
                        for (c, s) in frame.into_iter().enumerate() {
                            let sample = if s.is_finite() {
                                (s.clamp(-1.0, 1.0) as f64 * 2147483647.0) as i32
                            } else {
                                0
                            };
                            packet[4 + i * 8 + c * 4..8 + i * 8 + c * 4]
                                .copy_from_slice(&sample.to_le_bytes());
                        }
                    });
                    if n == 0 {
                        continue;
                    }
                    packet[..4].copy_from_slice(&(n as u32 * 8).to_le_bytes());
                    if socket
                        .as_mut()
                        .unwrap()
                        .write_all(&packet[..4 + n * 8])
                        .is_err()
                    {
                        ACTIVE.store(0, Ordering::Release);
                        socket = None;
                    }
                }
            })
            .expect("system return worker");
        tx
    });
    tx.send(target).is_ok()
}

pub fn publish(epoch: &mut usize, frame: [f32; 2]) {
    let current = ACTIVE.load(Ordering::Acquire);
    if current == 0 {
        return;
    }
    let fifo = FIFO.get().expect("initialized before activation");
    if *epoch != current {
        fifo.clear_from_producer();
        *epoch = current;
        READY.store(current, Ordering::Release);
    }
    let _ = fifo.push(&frame);
}
