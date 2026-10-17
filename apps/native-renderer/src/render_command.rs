//! Bounded control queue between stdin protocol parsing and the render worker.
//!
//! The protocol thread is the sole producer and the render worker is the sole
//! consumer. Its mutex never participates in the WASAPI callback or Engine state.

use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use super::Command;

const MAX_QUEUED_PCM_BYTES: usize = 16 * 1024 * 1024;

pub enum RenderCommand {
    Command(Command),
    /// One queue entry so mobile settings cannot be only partially enqueued.
    ObjectRendering {
        direct: bool,
        directional: bool,
    },
    /// Live asset replacement: no decoder restart, PCM flush or clock reset.
    HrtfPreset {
        set: Box<crate::hrtf::NativeHrtfSet>,
        wet: f32,
        direct: bool,
        directional: bool,
        reply: std::sync::mpsc::Sender<Result<(), String>>,
    },
    SpatialCueGain {
        update: Box<crate::live_spatial_cues::PreparedCueUpdate>,
        reply: std::sync::mpsc::Sender<Result<(), String>>,
    },
    NearField {
        settings: crate::near_field::Settings,
        reply: std::sync::mpsc::Sender<Result<(), String>>,
    },
    Room {
        settings: crate::cinema::Settings,
        profile: Option<std::sync::Arc<crate::cinema::RoomProfile>>,
        reply: std::sync::mpsc::Sender<Result<(), String>>,
    },
    Pcm {
        id: String,
        start: u64,
        samples: Vec<f32>,
    },
    PcmBatch {
        start: u64,
        entries: Vec<(String, Vec<f32>)>,
    },
    PcmFrame {
        start: u64,
        entries: Vec<(String, Vec<f32>)>,
        events: Vec<crate::NativeObjectEvent>,
    },
    /// Same transaction as the desktop F packet, with its ACK returned to JNI.
    PcmFrameWithAck {
        start: u64,
        entries: Vec<(String, Vec<f32>)>,
        events: Vec<crate::NativeObjectEvent>,
        reply: std::sync::mpsc::Sender<bool>,
    },
    HeadphoneFir {
        preamp: f32,
        left: Vec<f32>,
        right: Vec<f32>,
    },
}

impl RenderCommand {
    fn pcm_bytes(&self) -> usize {
        match self {
            Self::Command(Command::Feed { samples, .. }) => samples.len() * size_of::<f32>(),
            Self::Pcm { samples, .. } => samples.len() * size_of::<f32>(),
            Self::PcmBatch { entries, .. }
            | Self::PcmFrame { entries, .. }
            | Self::PcmFrameWithAck { entries, .. } => entries
                .iter()
                .map(|(_, samples)| samples.len() * size_of::<f32>())
                .sum(),
            Self::HeadphoneFir { left, right, .. } => (left.len() + right.len()) * size_of::<f32>(),
            _ => 0,
        }
    }
}

pub struct RenderCommandQueue {
    pending: Mutex<QueueState>,
    consumer: OnceLock<std::thread::Thread>,
    capacity: usize,
}

struct QueueState {
    commands: VecDeque<RenderCommand>,
    pcm_bytes: usize,
}

impl RenderCommandQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            pending: Mutex::new(QueueState {
                commands: VecDeque::with_capacity(capacity),
                pcm_bytes: 0,
            }),
            consumer: OnceLock::new(),
            capacity,
        }
    }

    /// Never waits for the render worker: stdin backpressure must not couple to
    /// the real-time output path. Callers receive a protocol rejection on full.
    pub fn push(&self, command: RenderCommand) -> Result<(), RenderCommand> {
        let bytes = command.pcm_bytes();
        let mut pending = self.pending.lock().expect("render command queue poisoned");
        if pending.commands.len() >= self.capacity
            || pending.pcm_bytes.saturating_add(bytes) > MAX_QUEUED_PCM_BYTES
        {
            return Err(command);
        }
        pending.pcm_bytes += bytes;
        pending.commands.push_back(command);
        if let Some(consumer) = self.consumer.get() { consumer.unpark(); }
        Ok(())
    }

    pub(super) fn pop(&self) -> Option<RenderCommand> {
        // Never block the render coordinator behind a preempted producer.
        // Contention defers commands, not already queued PCM rendering.
        let mut pending = match self.pending.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::WouldBlock) => return None,
            Err(std::sync::TryLockError::Poisoned(_)) => panic!("render command queue poisoned"),
        };
        let command = pending.commands.pop_front();
        if let Some(ref command) = command {
            pending.pcm_bytes = pending.pcm_bytes.saturating_sub(command.pcm_bytes());
        }
        command
    }

    pub(super) fn wait(&self, duration: Duration) {
        // The queue has exactly one consumer. park/unpark retains a wake token,
        // including notifications between the empty check and parking.
        let _ = self.consumer.set(std::thread::current());
        let pending = match self.pending.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::park_timeout(duration);
                return;
            }
            Err(std::sync::TryLockError::Poisoned(_)) => panic!("render command queue poisoned"),
        };
        if pending.commands.is_empty() {
            drop(pending);
            std::thread::park_timeout(duration);
        }
    }

    /// Debug access for hosts outside the crate (mobile facade tests).
    pub fn pending_len_for_debug(&self) -> usize {
        self.pending
            .try_lock()
            .map_or(0, |state| state.commands.len())
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.pending
            .lock()
            .expect("render command queue poisoned")
            .commands
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parked_consumer_wakes_without_reacquiring_a_condition_lock() {
        let queue = std::sync::Arc::new(RenderCommandQueue::new(4));
        let consumer_queue = queue.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let consumer = std::thread::spawn(move || {
            consumer_queue.consumer.set(std::thread::current()).unwrap();
            ready_tx.send(()).unwrap();
            consumer_queue.wait(Duration::from_secs(2));
            consumer_queue.pop()
        });
        ready_rx.recv().unwrap();
        queue.push(RenderCommand::Command(Command::Health)).unwrap_or_else(|_| panic!());
        assert!(matches!(consumer.join().unwrap(), Some(RenderCommand::Command(Command::Health))));
    }
    #[test]
    fn concurrent_producer_preserves_order_and_byte_accounting() {
        let queue = std::sync::Arc::new(RenderCommandQueue::new(16));
        let producer_queue = queue.clone();
        let producer = std::thread::spawn(move || {
            for index in 0..1000_u64 {
                let mut command = RenderCommand::Pcm {
                    id: "test".into(),
                    start: index,
                    samples: vec![0.25; 8],
                };
                loop {
                    match producer_queue.push(command) {
                        Ok(()) => break,
                        Err(returned) => {
                            command = returned;
                            std::thread::yield_now();
                        }
                    }
                }
            }
        });
        for expected in 0..1000_u64 {
            loop {
                if let Some(command) = queue.pop() {
                    let RenderCommand::Pcm { start, .. } = command else {
                        panic!()
                    };
                    assert_eq!(start, expected);
                    break;
                }
                std::thread::yield_now();
            }
        }
        producer.join().unwrap();
        assert_eq!(queue.pending.lock().unwrap().pcm_bytes, 0);
        assert_eq!(queue.len(), 0);
    }
    #[test]
    fn busy_producer_does_not_block_render_pop_or_lose_commands() {
        let queue = RenderCommandQueue::new(2);
        queue
            .push(RenderCommand::Pcm {
                id: "test".into(),
                start: 0,
                samples: vec![0.1; 8],
            })
            .unwrap_or_else(|_| panic!());
        let state = queue.pending.lock().unwrap();
        assert_eq!(state.pcm_bytes, 32);
        assert!(queue.pop().is_none());
        drop(state);
        assert!(matches!(queue.pop(), Some(RenderCommand::Pcm { .. })));
        assert_eq!(queue.pending.lock().unwrap().pcm_bytes, 0);
    }

    #[test]
    fn preserves_fifo_order_and_rejects_when_full() {
        let queue = RenderCommandQueue::new(1);
        assert!(queue.push(RenderCommand::Command(Command::Health)).is_ok());
        assert_eq!(queue.len(), 1);
        assert!(queue.push(RenderCommand::Command(Command::Health)).is_err());
        assert!(matches!(
            queue.pop(),
            Some(RenderCommand::Command(Command::Health))
        ));
    }
}
