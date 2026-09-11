//! A remote sink consumes an independent copy when WASAPI is available. The desktop
//! broker owns authentication/TLS; this socket is loopback-only and token gated.
use super::*;
use std::net::{SocketAddr, TcpStream};

pub const FRAMES: usize = 480;
pub static HOST_SELECTED: AtomicBool = AtomicBool::new(false);
// The render worker alone produces this second FIFO. Network backpressure
// cannot drain or stall the physical device FIFO.
pub static MIRROR_SELECTED: AtomicBool = AtomicBool::new(false);
static MIRROR_EPOCH: AtomicU64 = AtomicU64::new(0);
static MIRROR_READY: AtomicU64 = AtomicU64::new(0);
static MIRROR_OVERFLOW: AtomicBool = AtomicBool::new(false);
static MIRROR_FIFO: std::sync::OnceLock<stereo_fifo::StereoFifo> = std::sync::OnceLock::new();
pub static LOCAL_MUTED: AtomicBool = AtomicBool::new(true);
pub fn local_muted()->bool { HOST_SELECTED.load(Ordering::Acquire) && LOCAL_MUTED.load(Ordering::Acquire) }
pub fn mirror_fifo()-> &'static stereo_fifo::StereoFifo {MIRROR_FIFO.get_or_init(||stereo_fifo::StereoFifo::new(48000*6))}
pub fn select_mirror(selected:bool){if MIRROR_SELECTED.swap(selected,Ordering::AcqRel)!=selected{MIRROR_EPOCH.fetch_add(1,Ordering::AcqRel);}}
pub fn prepare_mirror(observed:&mut u64,reset:bool){let epoch=MIRROR_EPOCH.load(Ordering::Acquire);if reset||epoch!=*observed{mirror_fifo().clear_from_producer();MIRROR_OVERFLOW.store(false,Ordering::Release);*observed=epoch;MIRROR_READY.store(epoch,Ordering::Release);}}
pub fn publish_mirror(block:&[f32]){if MIRROR_SELECTED.load(Ordering::Acquire)&&mirror_fifo().push(block)!=block.len()/2{MIRROR_OVERFLOW.store(true,Ordering::Release);}}
pub fn mirror_ready()->bool{MIRROR_READY.load(Ordering::Acquire)==MIRROR_EPOCH.load(Ordering::Acquire)}
pub fn mirror_overflow()->bool{MIRROR_OVERFLOW.load(Ordering::Acquire)}
pub fn receiver() -> bool { std::env::var_os("SDA_REMOTE_RECEIVER").is_some() }

pub struct HostOutput {
    socket: TcpStream,
    pending: Vec<u8>,
    offset: usize,
    credits: usize,
}
impl HostOutput {
    pub fn connect(address: &str, token: &str) -> Result<Self, String> {
        let address: SocketAddr = address.parse().map_err(|_| "invalid local audio address")?;
        if !address.ip().is_loopback() || token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid local audio handshake".into());
        }
        let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(2)).map_err(|e|e.to_string())?;
        socket.set_write_timeout(Some(Duration::from_secs(2))).map_err(|e|e.to_string())?;
        socket.write_all(token.as_bytes()).map_err(|e|e.to_string())?;
        socket.set_nonblocking(true).map_err(|e|e.to_string())?;
        Ok(Self {socket,pending:Vec::with_capacity(5+FRAMES*8),offset:0,credits:0})
    }
    fn packet(&mut self, kind:u8, body:&[u8]) {
        self.pending.clear();self.offset=0;
        self.pending.push(kind);self.pending.extend_from_slice(&(body.len() as u32).to_le_bytes());
        self.pending.extend_from_slice(body);
    }
    pub fn tick(&mut self, fifo:&stereo_fifo::StereoFifo, t:&RuntimeTelemetry) -> Result<(),String> {
        // Complete a packet before acknowledging a flush: old audio must precede
        // the reset on the wire and can never reappear after it.
        if self.offset < self.pending.len() {
            match self.socket.write(&self.pending[self.offset..]) {
                Ok(0)=>return Err("remote audio disconnected".into()),
                Ok(n)=>self.offset+=n,
                Err(e) if e.kind()==io::ErrorKind::WouldBlock=>return Ok(()),
                Err(e)=>return Err(e.to_string()),
            }
            if self.offset < self.pending.len() {return Ok(());}
        }
        if fifo.apply_flush_from_consumer() {
            self.packet(b'R',&[]);return Ok(());
        }
        let mut requests=[0u8;128];
        match self.socket.read(&mut requests) {
            Ok(0)=>return Err("remote audio disconnected".into()),
            Ok(n)=>{
                if requests[..n].iter().any(|b|*b!=b'P') {return Err("invalid audio credit".into());}
                self.credits+=n;
                if self.credits>200 {return Err("audio credit limit".into());}
            },
            Err(e) if e.kind()==io::ErrorKind::WouldBlock=>{},
            Err(e)=>return Err(e.to_string()),
        }
        if self.credits==0 {return Ok(());}
        let enabled=t.callback_output_enabled.load(Ordering::Acquire);
        // A render stall is buffering, not permission to replace missing program
        // samples with silence. Keep the credit until a complete block exists.
        if enabled && fifo.available_read()<FRAMES {return Ok(());}
        self.credits-=1;
        let mut samples=[0u8;FRAMES*8];
        let popped=if enabled {fifo.pop_frames(FRAMES,|i,frame| {
            samples[i*8..i*8+4].copy_from_slice(&frame[0].to_le_bytes());
            samples[i*8+4..i*8+8].copy_from_slice(&frame[1].to_le_bytes());
        })}else{0};
        // Network demand, ultimately the receiver's device clock, advances playback.
        if popped>0 {t.callback_consumed_sample_pos.fetch_add(popped as u64,Ordering::Release);}
        self.packet(b'A',&samples);
        Ok(())
    }
}

/// Audio-only receiver: no HRTF, room processing, normalization or second mix.
pub fn read_receiver(fifo:Arc<stereo_fifo::StereoFifo>,t:Arc<RuntimeTelemetry>) -> io::Result<()> {
    let progress_fifo=fifo.clone();let progress_t=t.clone();
    thread::spawn(move || loop {
        let consumed=progress_t.callback_consumed_sample_pos.load(Ordering::Acquire);
        let queued=progress_fifo.available_read();
        let message=serde_json::json!({"type":"remoteProgress","consumed":consumed,"queued":queued,
            "buffering":queued<FRAMES});
        let mut out=io::stdout().lock();let _=writeln!(out,"{message}");let _=out.flush();drop(out);
        thread::sleep(Duration::from_millis(20));
    });
    let mut input=io::stdin().lock();
    let mut received=0u64;
    loop {
        let mut header=[0;5];input.read_exact(&mut header)?;
        let length=u32::from_le_bytes(header[1..5].try_into().unwrap()) as usize;
        match (header[0],length) {
            (b'A',n) if n==FRAMES*8=>{
                let mut body=[0u8;FRAMES*8];input.read_exact(&mut body)?;
                let samples:Vec<f32>=body.chunks_exact(4).map(|b|f32::from_le_bytes(b.try_into().unwrap())).collect();
                if samples.iter().any(|s|!s.is_finite()) {return Err(io::Error::other("non-finite remote audio"));}
                while fifo.available_write()<FRAMES {thread::sleep(Duration::from_millis(2));}
                if fifo.push(&samples)!=FRAMES {return Err(io::Error::other("remote FIFO overflow"));}
                received+=FRAMES as u64;
                if fifo.available_read()>=4*FRAMES {t.callback_output_enabled.store(true,Ordering::Release);}
            },
            (b'R',0)=>{
                // Stop consumption first, then apply a consumer-owned flush. Count
                // discarded queued samples as released credits, never as played audio.
                t.callback_output_enabled.store(false,Ordering::Release);

                let epoch=fifo.clear_from_producer();
                while !fifo.flush_acknowledged(epoch) {thread::sleep(Duration::from_millis(2));}
                t.callback_consumed_sample_pos.store(received,Ordering::Release);
            },
            _=>return Err(io::Error::other("invalid remote audio packet")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    #[test]
    fn remote_sink_preserves_float_bits_waits_for_credits_and_flushes_in_order() {
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();
        let token="ab".repeat(32);
        let mut sink=HostOutput::connect(&listener.local_addr().unwrap().to_string(),&token).unwrap();
        let (mut peer,_)=listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut auth=[0;64];peer.read_exact(&mut auth).unwrap();assert_eq!(auth,token.as_bytes());
        let fifo=stereo_fifo::StereoFifo::new(2048);let t=RuntimeTelemetry::default();
        t.callback_output_enabled.store(true,Ordering::Release);
        let samples:Vec<f32>=(0..FRAMES*2).map(|i|if i==0 {-0.0}else{(i as f32*0.03).sin()*0.9}).collect();
        fifo.push(&samples);
        sink.tick(&fifo,&t).unwrap();assert_eq!(fifo.available_read(),FRAMES);
        peer.write_all(b"P").unwrap();
        for _ in 0..8 {sink.tick(&fifo,&t).unwrap();thread::sleep(Duration::from_millis(1));}
        let mut head=[0;5];peer.read_exact(&mut head).unwrap();assert_eq!(head[0],b'A');
        assert_eq!(u32::from_le_bytes(head[1..].try_into().unwrap()) as usize,FRAMES*8);
        let mut bytes=vec![0;FRAMES*8];peer.read_exact(&mut bytes).unwrap();
        let expected:Vec<u8>=samples.iter().flat_map(|s|s.to_le_bytes()).collect();assert_eq!(bytes,expected);
        assert_eq!(t.callback_consumed_sample_pos.load(Ordering::Acquire),FRAMES as u64);
        // An empty render FIFO must not spend a credit or fabricate program audio.
        peer.write_all(b"P").unwrap();
        for _ in 0..4 {sink.tick(&fifo,&t).unwrap();thread::sleep(Duration::from_millis(1));}
        assert_eq!(sink.credits,1);assert_eq!(sink.offset,sink.pending.len());
        fifo.push(&samples);let epoch=fifo.clear_from_producer();
        for _ in 0..4 {sink.tick(&fifo,&t).unwrap();}
        assert!(fifo.flush_acknowledged(epoch));peer.read_exact(&mut head).unwrap();assert_eq!(head,[b'R',0,0,0,0]);
        assert_eq!(t.callback_consumed_sample_pos.load(Ordering::Acquire),FRAMES as u64);
    }
    #[test]
    fn remote_sink_sends_final_hrtf_output_not_source_pcm() {
        let count=4800;
        let pcm:Vec<f32>=(0..count).map(|i|0.01*(i as f32*0.173).sin()).collect();
        let render=|mode| {
            let mut engine=Engine::new(48000,2);
            engine.active_hrtf_set=Some(hrtf::NativeHrtfSet::load_calibrated(
                &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../web/public/hrtf/hrtf-set.json")).unwrap());
            engine.set_layout(vbap::LayoutId::Stereo2_0).unwrap();
            engine.rebuild_bus_renderer().unwrap();engine.output_active=true;engine.paused=false;engine.stereo_mode=mode;
            engine.headphone=headphone::HeadphoneCompensation::new(&[0.5,0.0],&[0.5,0.0],1.0).unwrap();
            for label in ["FrontLeft","FrontRight"] {
                let mut source=Source {kind:SourceKind::Bed,bed_label:Some(label.into()),gain:1.0,target_gain:1.0,
                    availability:1.0,availability_target:1.0,..Source::default()};
                Engine::set_source_route(&mut source,bed_route(label,&engine.vbap),0);
                source.samples.write(0,0,&if label=="FrontLeft"{pcm.clone()}else{vec![0.0;count]});
                engine.sources.insert(label.into(),source);
            }
            let mut result=vec![0.0;count*2];engine.render_into(&mut result,2);result
        };
        let original=render(StereoMode::Original);let rendered=render(StereoMode::Dry);
        assert!(original[8192..].chunks_exact(2).all(|f|f[1].abs()<1e-7));
        assert!(rendered[8192..].chunks_exact(2).any(|f|f[1].abs()>1e-5),"HRTF must reach the opposite ear");
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();let token="cd".repeat(32);
        let mut sink=HostOutput::connect(&listener.local_addr().unwrap().to_string(),&token).unwrap();let(mut peer,_)=listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();let mut auth=[0;64];peer.read_exact(&mut auth).unwrap();
        let fifo=stereo_fifo::StereoFifo::new(8192);let telemetry=RuntimeTelemetry::default();telemetry.callback_output_enabled.store(true,Ordering::Release);
        assert_eq!(fifo.push(&rendered),count);peer.write_all(&[b'P';10]).unwrap();
        for _ in 0..30 {sink.tick(&fifo,&telemetry).unwrap();thread::sleep(Duration::from_millis(1));}
        let mut actual=Vec::new();for _ in 0..10{let mut header=[0;5];peer.read_exact(&mut header).unwrap();assert_eq!(header[0],b'A');assert_eq!(u32::from_le_bytes(header[1..].try_into().unwrap()),3840);let mut body=[0;3840];peer.read_exact(&mut body).unwrap();actual.extend_from_slice(&body);}
        let expected:Vec<u8>=rendered.iter().flat_map(|v|v.to_le_bytes()).collect();assert_eq!(actual,expected,"remote sink changed rendered samples");
        if let Some(dir)=std::env::var_os("SDA_REMOTE_PROOF_DIR") {std::fs::create_dir_all(&dir).unwrap();std::fs::write(std::path::PathBuf::from(dir).join("rendered.f32"),&actual).unwrap();}
        eprintln!("4800 frames: remote TCP equals final HRTF + headphone FIR output bit-for-bit; opposite ear differs from original PCM");
    }

    #[test]
    fn stalled_remote_mirror_does_not_consume_or_block_local_fifo() {
        select_mirror(true);let mut epoch=u64::MAX;prepare_mirror(&mut epoch,false);
        assert!(mirror_ready());mirror_fifo().apply_flush_from_consumer();
        let local=stereo_fifo::StereoFifo::new(1024);let block=vec![0.125;FRAMES*2];
        for _ in 0..610 {assert_eq!(local.push(&block),FRAMES);publish_mirror(&block);assert_eq!(local.pop_frames(FRAMES,|_,v|assert_eq!(v,[0.125,0.125])),FRAMES);}
        assert!(mirror_overflow());assert_eq!(local.available_read(),0);
        select_mirror(false);prepare_mirror(&mut epoch,false);mirror_fifo().apply_flush_from_consumer();assert!(!mirror_overflow());
    }

    #[test]
    fn only_local_broker_addresses_are_allowed() {
        assert!(HostOutput::connect("192.0.2.1:1234",&"ab".repeat(32)).is_err());
        assert!(HostOutput::connect("127.0.0.1:1234","short").is_err());
    }
}
