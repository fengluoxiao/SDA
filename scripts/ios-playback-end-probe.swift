import Foundation

@main struct PlaybackEndProbe {
 static func main() {
  var state=PlaybackEndDrain()
  func check(_ expected:Bool,_ finished:Bool,_ decoded:UInt64,_ consumed:UInt64,_ fifo:Int,_ pending:Int,_ time:Double,_ latency:Double) {
   precondition(state.ready(inputFinished:finished,decoded:decoded,consumed:consumed,fifoFrames:fifo,pendingBatches:pending,now:time,outputLatency:latency)==expected)
  }
  check(false,false,48000,48000,0,0,1,0.25)
  check(false,true,48000,47999,0,0,2,0.25)
  check(false,true,48000,48000,1,0,3,0.25)
  check(false,true,48000,48000,0,1,4,0.25)
  check(false,true,48000,48000,0,0,5,0.25)
  check(false,true,48000,48000,0,0,5.125,0.25)
  check(true,true,48000,48000,0,0,5.25,0.25)
  state.reset() // pause/resume cannot spend its drain interval while paused
  check(false,true,48000,48000,0,0,20,0.25)
  check(false,true,48000,48000,0,0,20.25,0.5) // route latency increased
  check(true,true,48000,48000,0,0,20.5,0.5)
  check(false,true,96000,48000,0,1,21,0.25) // renderer still has a batch
  check(false,true,96000,96000,0,0,22,0.25)
  check(true,true,96000,96000,0,0,22.25,0.25)
  state.reset();check(true,true,0,0,0,0,30,0)
  print("PASS: EOF waits for queued audio, measured device latency and resume; route changes retain the tail")
 }
}
