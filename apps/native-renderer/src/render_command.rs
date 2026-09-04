//! Bounded control queue between stdin protocol parsing and the render worker.
//!
//! The protocol thread is the sole producer and the render worker is the sole
//! consumer. Its mutex never participates in the WASAPI callback or Engine state.

use std::{
    collections::VecDeque,
    sync::{Condvar, Mutex},
    time::Duration,
};

use super::Command;

const MAX_QUEUED_PCM_BYTES: usize = 16 * 1024 * 1024;

pub(super) enum RenderCommand {
    Command(Command),
    Pcm {
        id: String,
        start: u64,
        samples: Vec<f32>,
    },
    PcmBatch {
        start: u64,
        entries: Vec<(String, Vec<f32>)>,
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
            Self::PcmBatch { entries, .. } => entries
                .iter()
                .map(|(_, samples)| samples.len() * size_of::<f32>())
                .sum(),
            Self::HeadphoneFir { left, right, .. } => (left.len() + right.len()) * size_of::<f32>(),
            _ => 0,
        }
    }
}

pub(super) struct RenderCommandQueue {
    pending: Mutex<VecDeque<RenderCommand>>,
    available: Condvar,
    capacity: usize,
    queued_pcm_bytes: Mutex<usize>,
}

impl RenderCommandQueue {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            pending: Mutex::new(VecDeque::with_capacity(capacity)),
            available: Condvar::new(),
            capacity,
            queued_pcm_bytes: Mutex::new(0),
        }
    }

    /// Never waits for the render worker: stdin backpressure must not couple to
    /// the real-time output path. Callers receive a protocol rejection on full.
    pub(super) fn push(&self, command: RenderCommand) -> Result<(), RenderCommand> {
        let bytes = command.pcm_bytes();
        let mut pending = self.pending.lock().expect("render command queue poisoned");
        let mut queued_pcm_bytes = self
            .queued_pcm_bytes
            .lock()
            .expect("render command queue poisoned");
        if pending.len() >= self.capacity
            || queued_pcm_bytes.saturating_add(bytes) > MAX_QUEUED_PCM_BYTES
        {
            return Err(command);
        }
        *queued_pcm_bytes += bytes;
        pending.push_back(command);
        self.available.notify_one();
        Ok(())
    }

    pub(super) fn pop(&self) -> Option<RenderCommand> {
        let command = self
            .pending
            .lock()
            .expect("render command queue poisoned")
            .pop_front();
        if let Some(ref command) = command {
            let mut queued_pcm_bytes = self
                .queued_pcm_bytes
                .lock()
                .expect("render command queue poisoned");
            *queued_pcm_bytes = queued_pcm_bytes.saturating_sub(command.pcm_bytes());
        }
        command
    }

    pub(super) fn wait(&self, duration: Duration) {
        let pending = self.pending.lock().expect("render command queue poisoned");
        if pending.is_empty() {
            let _ = self.available.wait_timeout(pending, duration);
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.pending
            .lock()
            .expect("render command queue poisoned")
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
