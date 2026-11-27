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

pub const MAX_QUEUED_PCM_BYTES: usize = 16 * 1024 * 1024;

pub enum RenderCommand {
    Command(Command),
    TrackedHeadPose {orientation:[f32;4],sampled_at:std::time::Instant,output_latency_seconds:f64},
    ResetTrackingPrediction,
    TransportReset { epoch:u64, command:Command },
    /// One queue entry so mobile settings cannot be only partially enqueued.
    ObjectRendering {
        direct: bool,
        directional: bool,
    },
    /// Live asset replacement: no decoder restart, PCM flush or clock reset.
    AuthoredScene {scene:Box<crate::authored::Scene>,reply:std::sync::mpsc::Sender<Result<(),String>>},
    HrtfPreset {
        set: Box<crate::hrtf::NativeHrtfSet>,
        wet: f32,
        direct: bool,
        directional: bool,
        reply: std::sync::mpsc::Sender<Result<(), String>>,
    },
    ReferenceMonitor { update: Box<crate::live_spatial_cues::PreparedCueUpdate> },
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
    /// Full scene and compatible plain output committed at the same sample clock.
    /// The scene remains warm while only the final binaural output is bypassed.
    PcmFrameWithCompatibility {
        start: u64,
        entries: Vec<(String, Vec<f32>)>,
        events: Vec<crate::NativeObjectEvent>,
        compatibility: [Vec<f32>; 2],
        reply: Option<std::sync::mpsc::Sender<bool>>,
    },
    HeadphoneFir {
        preamp: f32,
        left: Vec<f32>,
        right: Vec<f32>,
    },
}

impl RenderCommand {
    fn valid_pose(&self)->bool {
        match self {
            Self::Command(Command::HeadPose{orientation})|Self::TrackedHeadPose{orientation,..}=>
                crate::spatial::normalize_quaternion(*orientation).is_some(),
            _=>false,
        }
    }
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
            Self::PcmFrameWithCompatibility { entries, compatibility, .. } =>
                (entries.iter().map(|(_, pcm)| pcm.len()).sum::<usize>()
                    + compatibility.iter().map(Vec::len).sum::<usize>()) * size_of::<f32>(),
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
    transport_epoch:u64,
    commands: VecDeque<RenderCommand>,
    pcm_bytes: usize,
}

impl RenderCommandQueue {
    /// Supersede queued source audio atomically while retaining live settings
    /// and their reply handles. Old source/transport commands cannot restart
    /// the abandoned clock after the new seek reset.
    pub fn reset_transport(&self, reset: Command) {
        let mut pending=self.pending.lock().expect("render command queue poisoned");
        pending.commands.retain(|item| match item {
            RenderCommand::Command(command)=> !matches!(command,
                Command::Feed{..}|Command::AddSource{..}|Command::RemoveSource{..}|
                Command::ObjectEvents{..}|Command::SetDownmixTrims{..}|
                Command::StartAt{..}|Command::Reset{..}|Command::SeekReset{..}|Command::SetProgramCodec{..}),
            RenderCommand::TransportReset{..}|RenderCommand::Pcm{..}|RenderCommand::PcmBatch{..}|RenderCommand::PcmFrame{..}|
            RenderCommand::PcmFrameWithAck{..}|RenderCommand::PcmFrameWithCompatibility{..}=>false,
            _=>true,
        });
        pending.pcm_bytes=pending.commands.iter().map(RenderCommand::pcm_bytes).sum();
        pending.transport_epoch=pending.transport_epoch.wrapping_add(1);
        let epoch=pending.transport_epoch;
        pending.commands.push_front(RenderCommand::TransportReset{epoch,command:reset});
        if let Some(consumer)=self.consumer.get(){consumer.unpark();}
    }

    pub(crate) fn publish_transport<T>(&self,epoch:u64,publish:impl FnOnce()->T)->Option<T>{
        let pending=self.pending.lock().expect("render command queue poisoned");
        (pending.transport_epoch==epoch).then(publish)
    }

    pub fn new(capacity: usize) -> Self {
        Self {
            pending: Mutex::new(QueueState {
                transport_epoch:0,
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
        // Only adjacent valid pose states may supersede one another. PCM,
        // reset, settings and reply-bearing commands remain ordering barriers.
        // Do this before the capacity check so stale poses cannot reject the
        // newest pose merely because they filled the queue themselves.
        if command.valid_pose() && pending.commands.back().is_some_and(RenderCommand::valid_pose) {
                *pending.commands.back_mut().unwrap() = command;
                if let Some(consumer) = self.consumer.get() {
                    consumer.unpark();
                }
                return Ok(());
        }
        if pending.commands.len() >= self.capacity
            || pending.pcm_bytes.saturating_add(bytes) > MAX_QUEUED_PCM_BYTES
        {
            return Err(command);
        }
        pending.pcm_bytes += bytes;
        pending.commands.push_back(command);
        if let Some(consumer) = self.consumer.get() {
            consumer.unpark();
        }
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
    fn pose_updates_coalesce_without_starving_pcm_or_crossing_audio_boundaries() {
        let queue = RenderCommandQueue::new(2);
        let mut latest = [0.0; 4];
        for i in 0..100 {
            let a = i as f32 * 0.005;
            latest = [0.0, 0.0, a.sin(), a.cos()];
            queue
                .push(RenderCommand::Command(Command::HeadPose {
                    orientation: latest,
                }))
                .unwrap_or_else(|_| panic!("pose burst exhausted the command queue at {i}"));
        }
        assert_eq!(queue.len(), 1);
        queue
            .push(RenderCommand::Pcm {
                id: "object".into(),
                start: 0,
                samples: vec![0.1; 8],
            })
            .unwrap_or_else(|_| panic!("obsolete poses starved PCM"));
        assert!(
            queue
                .push(RenderCommand::Command(Command::HeadPose {
                    orientation: [0.0, 0.0, 0.0, 1.0]
                }))
                .is_err(),
            "a pose must not be moved across a queued PCM boundary"
        );
        match queue.pop().unwrap() {
            RenderCommand::Command(Command::HeadPose { orientation }) => {
                assert_eq!(orientation, latest)
            }
            _ => panic!(),
        }
        assert!(matches!(
            queue.pop(),
            Some(RenderCommand::Pcm { start: 0, .. })
        ));
        assert_eq!(queue.pending.lock().unwrap().pcm_bytes, 0);
    }

    #[test]
    fn pose_coalescing_preserves_resets_and_invalid_command_boundaries() {
        let queue = RenderCommandQueue::new(8);
        let pose = |z: f32, w: f32| {
            RenderCommand::Command(Command::HeadPose {
                orientation: [0.0, 0.0, z, w],
            })
        };
        for command in [
            pose(0.0, 1.0),
            pose(f32::NAN, 1.0),
            pose(0.1, 0.995),
            RenderCommand::Command(Command::ClearHeadPose),
            pose(0.2, 0.98),
        ] {
            queue.push(command).unwrap_or_else(|_| panic!());
        }
        assert_eq!(queue.len(), 5);
        assert!(
            matches!(queue.pop(),Some(RenderCommand::Command(Command::HeadPose{orientation}))if orientation[2]==0.0)
        );
        assert!(
            matches!(queue.pop(),Some(RenderCommand::Command(Command::HeadPose{orientation}))if orientation[2].is_nan())
        );
        assert!(
            matches!(queue.pop(),Some(RenderCommand::Command(Command::HeadPose{orientation}))if orientation[2]==0.1)
        );
        assert!(matches!(
            queue.pop(),
            Some(RenderCommand::Command(Command::ClearHeadPose))
        ));
        assert!(
            matches!(queue.pop(),Some(RenderCommand::Command(Command::HeadPose{orientation}))if orientation[2]==0.2)
        );
    }
    #[test]
    fn timed_pose_keeps_latest_timestamp_and_prediction_reset_is_a_barrier() {
        let queue=RenderCommandQueue::new(8);
        let time=std::time::Instant::now();
        let timed=|ms|RenderCommand::TrackedHeadPose{orientation:[0.,0.,0.,1.],
            sampled_at:time+Duration::from_millis(ms),output_latency_seconds:0.03};
        for cmd in [timed(0),timed(1),RenderCommand::ResetTrackingPrediction,timed(2),timed(3)]{
            assert!(queue.push(cmd).is_ok());
        }
        assert_eq!(queue.len(),3);
        assert!(matches!(queue.pop(),Some(RenderCommand::TrackedHeadPose{sampled_at,..})if sampled_at==time+Duration::from_millis(1)));
        assert!(matches!(queue.pop(),Some(RenderCommand::ResetTrackingPrediction)));
        assert!(matches!(queue.pop(),Some(RenderCommand::TrackedHeadPose{sampled_at,..})if sampled_at==time+Duration::from_millis(3)));
    }
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
    #[test]
    fn seek_epoch_retires_old_audio_and_preserves_live_settings() {
        let queue=RenderCommandQueue::new(16);
        assert!(queue.push(RenderCommand::Pcm{id:"old".into(),start:0,samples:vec![0.5;1024]}).is_ok());
        assert!(queue.push(RenderCommand::ObjectRendering{direct:true,directional:true}).is_ok());
        queue.reset_transport(Command::SeekReset{origin:4000,target:10000});
        assert!(matches!(queue.pop(),Some(RenderCommand::TransportReset{epoch:1,..})));
        assert!(matches!(queue.pop(),Some(RenderCommand::ObjectRendering{direct:true,directional:true})));
        assert!(queue.pop().is_none());assert_eq!(queue.pending.lock().unwrap().pcm_bytes,0);
        let published=std::sync::atomic::AtomicUsize::new(0);
        assert!(queue.publish_transport(0,||published.fetch_add(1,std::sync::atomic::Ordering::Relaxed)).is_none());
        assert!(queue.publish_transport(1,||published.fetch_add(1,std::sync::atomic::Ordering::Relaxed)).is_some());
        queue.reset_transport(Command::SeekReset{origin:8000,target:20000});
        assert!(queue.publish_transport(1,||published.fetch_add(1,std::sync::atomic::Ordering::Relaxed)).is_none());
        assert!(queue.publish_transport(2,||published.fetch_add(1,std::sync::atomic::Ordering::Relaxed)).is_some());
        assert_eq!(published.load(std::sync::atomic::Ordering::Relaxed),2,"abandoned render blocks cannot reach FIFO");
    }

}
