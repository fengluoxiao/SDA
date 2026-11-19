import UIKit
import AVFoundation
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
 let surface: PersonalHrtfReviewGeometry
 let maximumSupportDistance: Double
}
protocol PersonalHeadReconstructor: AnyObject {
 var progress: ((Int, String) -> Void)? { get set }
 var captureReadiness: ((Bool, String) -> Void)? { get set }
 var captureLimit: ((Int, String) -> Void)? { get set }
 func start(completion: @escaping (Result<PersonalHeadMeshCandidate,Error>) -> Void) throws
 func accumulate(_ frame: PersonalHeadDepthFrame)
 func finish()
 func cancel()
}
extension PersonalHeadReconstructor {
 var captureReadiness: ((Bool, String) -> Void)? { get { nil } set {} }
 var captureLimit: ((Int, String) -> Void)? { get { nil } set {} }
}
// Front TrueDepth frames go straight to the on-device reconstruction engine.
// ARFaceGeometry is never substituted for measured head or ear surfaces.
final class PersonalHeadScanController: UIViewController {
 static func supported() -> Bool {
  AVCaptureDevice.default(.builtInTrueDepthCamera,for:.video,position:.front) != nil
 }
 static func discardAbandonedScans() {
  let root = FileManager.default.temporaryDirectory.appendingPathComponent("SDAHeadScan",isDirectory:true)
  try? FileManager.default.removeItem(at:root)
 }
 private let reconstruction: PersonalHeadReconstructor
 private let camera = PersonalHeadDepthCapture()
 private let preview = UIView(frame:.zero)
 private let message = UILabel(), countLabel = UILabel(), coverageLabel = UILabel()
 private let finishButton = UIButton(type:.system)
 private let frameLock = NSLock()
 private var acceptingFrames = false // protected by frameLock
 private var finished = false, permissionCancelled = false // main queue
 private var capturePaused = false, processing = false // main queue
 private var reason: Error?
 var complete: ((Result<PersonalHeadMeshCandidate,Error>) -> Void)?
 init(reconstruction: PersonalHeadReconstructor) {
  self.reconstruction = reconstruction; super.init(nibName:nil,bundle:nil)
 }
 required init?(coder:NSCoder) { fatalError("Use the front TrueDepth initializer") }
 func cancel() { stop("已取消，临时头耳测量将被删除") }
 override func viewDidLoad() {
  super.viewDidLoad(); view.backgroundColor = .black
  preview.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(preview)
  preview.layer.addSublayer(camera.previewLayer)
  message.textColor = .white; message.numberOfLines = 0; message.textAlignment = .center
  message.font = .preferredFont(forTextStyle:.body)
  message.text = "仅使用前置 TrueDepth，在 iPhone 上实时重建。需要他人协助：移开遮耳头发，保持头部和表情稳定；缓慢移动手机，保持相邻画面重叠，覆盖两耳、后脑、头顶和下方。保持约 25–60 厘米距离。"
  countLabel.textColor = .white; countLabel.textAlignment = .center; countLabel.numberOfLines = 0
  coverageLabel.textColor = .white; coverageLabel.textAlignment = .center; coverageLabel.numberOfLines = 0
  coverageLabel.font = .preferredFont(forTextStyle:.footnote)
  coverageLabel.text = "头部：待采集 · 左耳：待采集 · 右耳：待采集\n区域识别不代表耳廓精度合格；仍需覆盖后脑和头顶"
  finishButton.setTitle("完成采集，在本机处理",for:.normal); finishButton.isEnabled = false
  finishButton.addTarget(self,action:#selector(finishScan),for:.touchUpInside)
  let cancel = UIButton(type:.system); cancel.setTitle("取消并删除",for:.normal)
  cancel.addTarget(self,action:#selector(cancelScan),for:.touchUpInside)
  let stack = UIStackView(arrangedSubviews:[message,countLabel,coverageLabel,finishButton,cancel]); stack.axis = .vertical; stack.spacing = 14
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
  reconstruction.progress = { [weak self] count,text in
   DispatchQueue.main.async {
    guard let self, !self.finished, !self.capturePaused || self.processing else { return }
    self.countLabel.text = "已配准 \(count) 帧 · \(text)"

   }
  }
  reconstruction.captureReadiness = { [weak self] ready,text in
   DispatchQueue.main.async {
    guard let self, !self.finished, !self.processing, !self.capturePaused else { return }
    self.coverageLabel.text = text + "\n区域识别不代表耳廓精度合格；仍需覆盖后脑和头顶"
    self.frameLock.lock(); let scanning = self.acceptingFrames; self.frameLock.unlock()
    self.finishButton.isEnabled = scanning && ready
   }
  }
  reconstruction.captureLimit = { [weak self] count,text in
   DispatchQueue.main.async {
    guard let self, !self.finished, !self.processing else { return }
    self.capturePaused = true
    self.stopFrames()
    self.message.text = text
    self.countLabel.text = "已保留本次测量，可处理现有模型或取消删除"
    self.finishButton.setTitle("处理已采集的模型",for:.normal)
    self.finishButton.isEnabled = count > 0
   }
  }
  do { try reconstruction.start { [weak self] result in
   DispatchQueue.main.async { self?.end(result) }
  } } catch { end(.failure(error)); return }
  guard Self.supported() else { stop("此设备没有可用的前置 TrueDepth 深度相机"); return }
  frameLock.lock(); acceptingFrames = true; frameLock.unlock()
  camera.start(frame:{ [weak self] frame in
   guard let self else { return }; self.frameLock.lock(); let accept = self.acceptingFrames; self.frameLock.unlock()
   if accept { self.reconstruction.accumulate(frame) }
  },failure:{ [weak self] error in self?.stop(error.localizedDescription) })
 }
 override func viewDidLayoutSubviews() {
  super.viewDidLayoutSubviews(); camera.previewLayer.frame = preview.bounds
  if let connection = camera.previewLayer.connection, connection.isVideoOrientationSupported,
     let orientation = view.window?.windowScene?.interfaceOrientation {
   switch orientation {
   case .landscapeLeft: connection.videoOrientation = .landscapeLeft
   case .landscapeRight: connection.videoOrientation = .landscapeRight
   case .portraitUpsideDown: connection.videoOrientation = .portraitUpsideDown
   default: connection.videoOrientation = .portrait
   }
  }
 }
 override func viewDidDisappear(_ animated: Bool) {
  super.viewDidDisappear(animated); permissionCancelled = true; stopFrames()
  UIApplication.shared.isIdleTimerDisabled = false
  if !finished { reconstruction.cancel() }
 }
 private func stopFrames() {
  frameLock.lock(); acceptingFrames = false; frameLock.unlock(); camera.stop()
 }
 @objc private func finishScan() {
  guard !finished, !processing else { return }; processing = true; permissionCancelled = true; stopFrames()
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
  stopFrames(); reconstruction.progress = nil; reconstruction.captureLimit = nil; reconstruction.captureReadiness = nil; UIApplication.shared.isIdleTimerDisabled = false
  let answer: Result<PersonalHeadMeshCandidate,Error>
  if let reason {
   if case .success(let candidate) = result { try? FileManager.default.removeItem(at:candidate.directory) }
   answer = .failure(reason)
  } else { answer = result }
  let done = complete; complete = nil
  dismiss(animated:true) { done?(answer) }
 }
}
