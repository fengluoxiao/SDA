import UIKit
import ARKit
import AVFoundation
import SceneKit
import simd

// A candidate is NOT an acoustic head mesh or an installable HRTF. The local
// pipeline owns its temporary directory after successful reconstruction.
struct PersonalHeadMeshCandidate {
 let directory: URL
 let support: URL
 let mesh: URL
 let cameraPositions: [SIMD3<Float>]
 let gravity: SIMD3<Float>
 let points: Int
 let geometryScreenVersion: Int
 let maximumSupportDistance: Double
}
protocol PersonalHeadReconstructor: AnyObject {
 var progress: ((Int, String) -> Void)? { get set }
 func start(completion: @escaping (Result<PersonalHeadMeshCandidate,Error>) -> Void) throws
 func accumulate(_ frame: ARFrame)
 func finish()
 func cancel()
}
// Front TrueDepth frames go straight to the on-device reconstruction engine.
// ARFaceGeometry is never substituted for measured head or ear surfaces.
final class PersonalHeadScanController: UIViewController, ARSessionDelegate {
 static func supported() -> Bool {
  ARFaceTrackingConfiguration.isSupported &&
   AVCaptureDevice.default(.builtInTrueDepthCamera,for:.video,position:.front) != nil
 }
 static func discardAbandonedScans() {
  let root = FileManager.default.temporaryDirectory.appendingPathComponent("SDAHeadScan",isDirectory:true)
  try? FileManager.default.removeItem(at:root)
 }
 private let reconstruction: PersonalHeadReconstructor
 private let session = ARSession()
 private let worker = DispatchQueue(label:"app.sda.headscan",qos:.userInitiated)
 private let preview = ARSCNView(frame:.zero)
 private let message = UILabel(), countLabel = UILabel()
 private let finishButton = UIButton(type:.system)
 private let frameLock = NSLock()
 private var acceptingFrames = false // protected by frameLock
 private var finished = false, permissionCancelled = false // main queue
 private var reason: Error?
 var complete: ((Result<PersonalHeadMeshCandidate,Error>) -> Void)?
 init(reconstruction: PersonalHeadReconstructor) {
  self.reconstruction = reconstruction; super.init(nibName:nil,bundle:nil)
 }
 required init?(coder:NSCoder) { fatalError("Use the front TrueDepth initializer") }
 func cancel() { stop("已取消，临时头耳测量将被删除") }
 override func viewDidLoad() {
  super.viewDidLoad(); view.backgroundColor = .black
  preview.session = session; preview.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(preview)
  message.textColor = .white; message.numberOfLines = 0; message.textAlignment = .center
  message.font = .preferredFont(forTextStyle:.body)
  message.text = "仅使用前置 TrueDepth，在 iPhone 上实时重建。需要他人协助：移开遮耳头发，保持头部和表情稳定；缓慢移动手机，保持相邻画面重叠，覆盖两耳、后脑、头顶和下方。保持约 25–60 厘米距离。"
  countLabel.textColor = .white; countLabel.textAlignment = .center; countLabel.numberOfLines = 0
  finishButton.setTitle("完成采集，在本机处理",for:.normal); finishButton.isEnabled = false
  finishButton.addTarget(self,action:#selector(finishScan),for:.touchUpInside)
  let cancel = UIButton(type:.system); cancel.setTitle("取消并删除",for:.normal)
  cancel.addTarget(self,action:#selector(cancelScan),for:.touchUpInside)
  let stack = UIStackView(arrangedSubviews:[message,countLabel,finishButton,cancel]); stack.axis = .vertical; stack.spacing = 14
  stack.backgroundColor = UIColor.black.withAlphaComponent(0.65); stack.isLayoutMarginsRelativeArrangement = true
  stack.layoutMargins = UIEdgeInsets(top:16,left:18,bottom:12,right:18); stack.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(stack)
  NSLayoutConstraint.activate([
   preview.topAnchor.constraint(equalTo:view.topAnchor),preview.bottomAnchor.constraint(equalTo:view.bottomAnchor),
   preview.leadingAnchor.constraint(equalTo:view.leadingAnchor),preview.trailingAnchor.constraint(equalTo:view.trailingAnchor),
   stack.leadingAnchor.constraint(equalTo:view.safeAreaLayoutGuide.leadingAnchor,constant:12),
   stack.trailingAnchor.constraint(equalTo:view.safeAreaLayoutGuide.trailingAnchor,constant:-12),
   stack.bottomAnchor.constraint(equalTo:view.safeAreaLayoutGuide.bottomAnchor,constant:-8)
  ])
  isModalInPresentation = true; UIApplication.shared.isIdleTimerDisabled = true
  session.delegate = self; session.delegateQueue = worker
  reconstruction.progress = { [weak self] count,text in
   DispatchQueue.main.async {
    guard let self, !self.finished else { return }
    self.countLabel.text = "已配准 \(count) 帧 · \(text)"
    self.frameLock.lock(); let scanning = self.acceptingFrames; self.frameLock.unlock()
    self.finishButton.isEnabled = scanning && count >= 72
   }
  }
  do { try reconstruction.start { [weak self] result in
   DispatchQueue.main.async { self?.end(result) }
  } } catch { end(.failure(error)); return }
  AVCaptureDevice.requestAccess(for:.video) { [weak self] allowed in
   DispatchQueue.main.async {
    guard let self, !self.permissionCancelled, !self.finished else { return }
    guard allowed else { self.stop("需要相机权限才能采集头耳几何，可以在系统设置中开启"); return }
    guard Self.supported() else { self.stop("此设备没有可用的前置 TrueDepth 深度相机"); return }
    let config = ARFaceTrackingConfiguration(); config.isWorldTrackingEnabled = false
    config.isLightEstimationEnabled = false; config.maximumNumberOfTrackedFaces = 1
    self.frameLock.lock(); self.acceptingFrames = true; self.frameLock.unlock()
    self.session.run(config,options:[.resetTracking,.removeExistingAnchors])
   }
  }
 }
 override func viewDidDisappear(_ animated: Bool) {
  super.viewDidDisappear(animated); permissionCancelled = true; stopFrames()
  UIApplication.shared.isIdleTimerDisabled = false
  if !finished { reconstruction.cancel() }
 }
 private func stopFrames() {
  frameLock.lock(); acceptingFrames = false; frameLock.unlock(); session.pause()
 }
 func session(_ session: ARSession,didUpdate frame: ARFrame) {
  frameLock.lock(); let accept = acceptingFrames; frameLock.unlock()
  if accept { reconstruction.accumulate(frame) }
 }
 func session(_ session: ARSession,didFailWithError error: Error) {
  DispatchQueue.main.async { [weak self] in self?.stop("前置深度采集失败：\(error.localizedDescription)") }
 }
 func sessionWasInterrupted(_ session: ARSession) {
  DispatchQueue.main.async { [weak self] in self?.stop("采集已中断，请保持应用在前台重新采集") }
 }
 @objc private func finishScan() {
  guard !finished else { return }; permissionCancelled = true; stopFrames()
  finishButton.isEnabled = false; message.text = "正在 iPhone 上完成配准与重建，请保持应用在前台。"
  reconstruction.finish()
 }
 @objc private func cancelScan() { cancel() }
 private func stop(_ message: String) {
  guard !finished else { return }; permissionCancelled = true; reason = PersonalHrtfError.message(message)
  stopFrames(); finishButton.isEnabled = false; reconstruction.cancel()
 }
 private func end(_ result: Result<PersonalHeadMeshCandidate,Error>) {
  guard !finished else { return }; finished = true; permissionCancelled = true
  stopFrames(); reconstruction.progress = nil; UIApplication.shared.isIdleTimerDisabled = false
  let answer: Result<PersonalHeadMeshCandidate,Error>
  if let reason {
   if case .success(let candidate) = result { try? FileManager.default.removeItem(at:candidate.directory) }
   answer = .failure(reason)
  } else { answer = result }
  let done = complete; complete = nil
  dismiss(animated:true) { done?(answer) }
 }
}
