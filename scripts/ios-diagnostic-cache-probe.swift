import Foundation

@main struct DiagnosticCacheProbe {
 static func main() {
  let cache=DiagnosticSnapshotCache(),started=DispatchSemaphore(value:0),release=DispatchSemaphore(value:0)
  _ = cache.read { ["generation":1] }
  Thread.sleep(forTimeInterval:0.1)
  let old=cache.read { started.signal();release.wait();return ["generation":2] }
  precondition(old["generation"] as? Int == 1)
  precondition(started.wait(timeout:.now()+2) == .success)
  let before=ProcessInfo.processInfo.systemUptime
  for _ in 0..<100 {
   let value=cache.read { fatalError("A second capture started while the player was blocked") }
   precondition(value["generation"] as? Int == 1 && value["snapshotPending"] as? Bool == true)
  }
  precondition(ProcessInfo.processInfo.systemUptime-before < 1)
  cache.reset()
  let empty=cache.read { fatalError("Reset spawned another blocked capture") }
  precondition(empty["snapshotAvailable"] as? Bool == false)
  release.signal();Thread.sleep(forTimeInterval:0.1)
  let fresh=cache.read { ["generation":3] }
  precondition(fresh["generation"] == nil,"Old capture escaped its configuration epoch")
  Thread.sleep(forTimeInterval:0.1)
  precondition(cache.read { ["generation":3] }["generation"] as? Int == 3)
  print("PASS: blocked player never blocks diagnostic reads; capture is bounded and stale epochs are discarded")
 }
}
