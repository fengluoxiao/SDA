import ARKit
import AVFoundation
import SceneKit
import UIKit

typealias PersonalHeadDepthFrame = ARFrame

final class PersonalHeadDepthCapture: NSObject, ARSessionDelegate {
 let preview = ARSCNView(frame:.zero)
 private let session = ARSession()
 private let queue = DispatchQueue(label:"app.sda.faceframe.camera",qos:.userInitiated)
 private let lock = NSLock()
 private var stopped = false
 private var deliver: ((ARFrame) -> Void)?
 private var failure: ((Error) -> Void)?

 override init() {
  super.init()
  preview.session = session; preview.scene = SCNScene()
  session.delegateQueue = queue; session.delegate = self
 }
 func start(frame:@escaping(ARFrame)->Void,failure:@escaping(Error)->Void) {
  lock.lock(); guard !stopped else { lock.unlock(); return }
  deliver = frame; self.failure = failure; lock.unlock()
  AVCaptureDevice.requestAccess(for:.video) { [weak self] allowed in
   DispatchQueue.main.async { [weak self] in
    guard let self else { return }
    self.lock.lock(); let active = !self.stopped; self.lock.unlock()
    guard active else { return }
    guard allowed, ARFaceTrackingConfiguration.isSupported,
          AVCaptureDevice.default(.builtInTrueDepthCamera,for:.video,position:.front) != nil else {
     self.report(PersonalHrtfError.message("需要前置 TrueDepth 和相机权限")); return
    }
    let config = ARFaceTrackingConfiguration()
    config.maximumNumberOfTrackedFaces = 1; config.isLightEstimationEnabled = false
    config.isWorldTrackingEnabled = false; config.providesAudioData = false
    self.session.run(config,options:[.resetTracking,.removeExistingAnchors])
   }
  }
 }
 func stop() {
  lock.lock(); stopped = true; deliver = nil; failure = nil; lock.unlock()
  session.pause()
 }
 func session(_ session:ARSession,didUpdate frame:ARFrame) {
  lock.lock(); let callback = deliver; lock.unlock(); callback?(frame)
 }
 func session(_ session:ARSession,didFailWithError error:Error) { report(error) }
 func sessionWasInterrupted(_ session:ARSession) { report(PersonalHrtfError.message("前置 TrueDepth 采集已中断")) }
 private func report(_ error:Error) {
  DispatchQueue.main.async { [weak self] in
   guard let self else { return }; self.lock.lock(); let callback = self.failure; self.lock.unlock()
   self.stop(); callback?(error)
  }
 }
}
