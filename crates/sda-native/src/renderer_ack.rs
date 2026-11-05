//! A pending renderer reply must not keep a source alive after output shutdown.
use std::{sync::{mpsc::{Receiver,RecvTimeoutError},atomic::Ordering},time::{Duration,Instant}};
use sda_native_renderer::RuntimeTelemetry;
pub(crate) fn wait<T>(receiver:&Receiver<T>,telemetry:&RuntimeTelemetry,timeout:Duration,message:&str)->Result<T,String>{
 let deadline=Instant::now()+timeout;
 loop{
  if telemetry.shutdown_requested.load(Ordering::Acquire){return Err("native renderer stopped before acknowledgement".into());}
  let remaining=deadline.saturating_duration_since(Instant::now());if remaining.is_zero(){return Err(message.into());}
  match receiver.recv_timeout(remaining.min(Duration::from_millis(20))){Ok(value)=>return Ok(value),Err(RecvTimeoutError::Disconnected)=>return Err("native renderer acknowledgement channel closed".into()),Err(RecvTimeoutError::Timeout)=>{}}
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn renderer_shutdown_cancels_a_pending_reply(){let(_tx,rx)=std::sync::mpsc::channel::<bool>();let telemetry=RuntimeTelemetry::default();telemetry.shutdown_requested.store(true,Ordering::Release);let start=Instant::now();assert!(wait(&rx,&telemetry,Duration::from_secs(60),"timeout").unwrap_err().contains("stopped"));assert!(start.elapsed()<Duration::from_secs(1));}
 #[test]fn renderer_replies_and_closed_channels_keep_their_results(){let(tx,rx)=std::sync::mpsc::channel();let telemetry=RuntimeTelemetry::default();tx.send(Ok::<_,String>(7)).unwrap();assert_eq!(wait(&rx,&telemetry,Duration::from_secs(1),"timeout").unwrap().unwrap(),7);drop(tx);assert!(wait(&rx,&telemetry,Duration::from_secs(1),"timeout").unwrap_err().contains("closed"));}
}
