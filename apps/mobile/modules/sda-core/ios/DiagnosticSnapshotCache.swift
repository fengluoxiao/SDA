import Foundation

/// The network queue never waits for a decoder, player lock or accelerator.
/// Only one capture can be in flight, even if diagnostics is toggled repeatedly.
final class DiagnosticSnapshotCache {
 private let lock=NSLock()
 private let worker=DispatchQueue(label:"sda.diagnostics.capture",qos:.utility)
 private var cached:[String:Any]?
 private var capturedAt:TimeInterval=0
 private var inFlight=false
 private var epoch=0
 func reset() {
  lock.lock();defer{lock.unlock()}
  epoch += 1;cached=nil;capturedAt=0
 }
 func read(capture:@escaping () -> [String:Any]) -> [String:Any] {
  lock.lock();defer{lock.unlock()}
  if !inFlight {
   inFlight=true;let expected=epoch
   worker.async { [self] in
    let value=capture()
    lock.lock();defer{lock.unlock()}
    inFlight=false
    if epoch == expected { cached=value;capturedAt=ProcessInfo.processInfo.systemUptime }
   }
  }
  var result=cached ?? ["schema":1,"time":Date().timeIntervalSince1970]
  result["snapshotPending"]=inFlight
  result["snapshotAvailable"]=cached != nil
  result["snapshotAgeMs"]=cached == nil ? -1 : max(0,(ProcessInfo.processInfo.systemUptime-capturedAt)*1000)
  return result
 }
}
