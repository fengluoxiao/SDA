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
 var captureGuide: ((PersonalHeadCaptureGuide) -> Void)? { get set }
 var captureReadiness: ((Bool, String) -> Void)? { get set }
 var captureLimit: ((Int, String) -> Void)? { get set }
 func start(completion: @escaping (Result<PersonalHeadMeshCandidate,Error>) -> Void) throws
 func accumulate(_ frame: PersonalHeadDepthFrame)
 func finish()
 func cancel()
}
extension PersonalHeadReconstructor {
 var captureGuide: ((PersonalHeadCaptureGuide) -> Void)? { get { nil } set {} }
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
 private let stageTitle = UILabel(), targetIcon = UIImageView(), stageProgress = UIProgressView(progressViewStyle:.default)
 private let speaker = AVSpeechSynthesizer()
 private var shownStage: PersonalHeadCaptureGuide.Stage?
 private let feedback = UINotificationFeedbackGenerator()
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
  message.text = PersonalHeadCaptureGuide().instruction
  stageTitle.font = .preferredFont(forTextStyle:.title2); stageTitle.textColor = .white; stageTitle.textAlignment = .center
  stageTitle.adjustsFontForContentSizeCategory = true
  targetIcon.tintColor = .white; targetIcon.contentMode = .scaleAspectFit
  targetIcon.heightAnchor.constraint(equalToConstant:56).isActive = true
  stageProgress.progressTintColor = .systemGreen
  stageProgress.trackTintColor = UIColor.white.withAlphaComponent(0.2)

  countLabel.textColor = .white; countLabel.textAlignment = .center; countLabel.numberOfLines = 0
  coverageLabel.textColor = .white; coverageLabel.textAlignment = .center; coverageLabel.numberOfLines = 0
  coverageLabel.font = .preferredFont(forTextStyle:.footnote)
  coverageLabel.text = PersonalHeadCaptureGuide().status
  finishButton.setTitle("采集完成后继续",for:.normal); finishButton.isEnabled = false
  finishButton.addTarget(self,action:#selector(finishScan),for:.touchUpInside)
  let cancel = UIButton(type:.system); cancel.setTitle("取消并删除",for:.normal)
  cancel.addTarget(self,action:#selector(cancelScan),for:.touchUpInside)
  let stack = UIStackView(arrangedSubviews:[targetIcon,stageTitle,message,stageProgress,coverageLabel,countLabel,finishButton,cancel]); stack.axis = .vertical; stack.spacing = 10
  stack.backgroundColor = UIColor.black.withAlphaComponent(0.65); stack.isLayoutMarginsRelativeArrangement = true
  stack.layoutMargins = UIEdgeInsets(top:16,left:18,bottom:12,right:18)
  let scroll = UIScrollView(); scroll.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(scroll)
  stack.translatesAutoresizingMaskIntoConstraints = false; scroll.addSubview(stack)
  NSLayoutConstraint.activate([
   preview.topAnchor.constraint(equalTo:view.topAnchor),preview.bottomAnchor.constraint(equalTo:view.bottomAnchor),
   preview.leadingAnchor.constraint(equalTo:view.leadingAnchor),preview.trailingAnchor.constraint(equalTo:view.trailingAnchor),
   scroll.leadingAnchor.constraint(equalTo:view.safeAreaLayoutGuide.leadingAnchor,constant:12),
   scroll.trailingAnchor.constraint(equalTo:view.safeAreaLayoutGuide.trailingAnchor,constant:-12),
   scroll.bottomAnchor.constraint(equalTo:view.safeAreaLayoutGuide.bottomAnchor,constant:-8),
   scroll.heightAnchor.constraint(equalTo:view.safeAreaLayoutGuide.heightAnchor,multiplier:0.58),
   stack.topAnchor.constraint(equalTo:scroll.contentLayoutGuide.topAnchor),stack.bottomAnchor.constraint(equalTo:scroll.contentLayoutGuide.bottomAnchor),
   stack.leadingAnchor.constraint(equalTo:scroll.contentLayoutGuide.leadingAnchor),stack.trailingAnchor.constraint(equalTo:scroll.contentLayoutGuide.trailingAnchor),
   stack.widthAnchor.constraint(equalTo:scroll.frameLayoutGuide.widthAnchor)
  ])
  isModalInPresentation = true; UIApplication.shared.isIdleTimerDisabled = true
  showGuide(PersonalHeadCaptureGuide())
  reconstruction.captureGuide = { [weak self] state in
   DispatchQueue.main.async {
    guard let self, !self.finished, !self.processing, !self.capturePaused else { return }
    self.showGuide(state)

   }
  }
  reconstruction.progress = { [weak self] count,text in
   DispatchQueue.main.async {
    guard let self, !self.finished, !self.capturePaused || self.processing else { return }
    self.countLabel.text = "已配准 \(count) 帧 · \(text)"

   }
  }
  reconstruction.captureReadiness = { [weak self] ready,text in
   DispatchQueue.main.async {
    guard let self, !self.finished, !self.processing, !self.capturePaused else { return }
    self.coverageLabel.text = text
    self.frameLock.lock(); let scanning = self.acceptingFrames; self.frameLock.unlock()
    self.finishButton.isEnabled = ready && (scanning || self.shownStage == .complete)
    if ready { self.stopFrames() }
   }
  }
  reconstruction.captureLimit = { [weak self] count,text in
   DispatchQueue.main.async {
    guard let self, !self.finished, !self.processing else { return }
    self.capturePaused = true
    self.stopFrames()
    self.message.text = text
    self.countLabel.text = "已保留本次测量，可处理现有模型或取消删除"
    self.speaker.stopSpeaking(at:.immediate)
    self.finishButton.setTitle("三个步骤未完成，请重新采集",for:.normal)
    self.finishButton.isEnabled = false
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
 private func showGuide(_ state: PersonalHeadCaptureGuide) {
  stageTitle.text = state.title; message.text = state.instruction
  coverageLabel.text = state.status; stageProgress.setProgress(state.fraction,animated:true)
  targetIcon.image = UIImage(systemName:state.complete ? "checkmark.circle.fill" : (state.stage == .face ? "faceid" : "ear"))
  targetIcon.transform = state.stage == .rightEar ? CGAffineTransform(scaleX:-1,y:1) : .identity
  guard shownStage != state.stage else { return }
  if shownStage != nil { feedback.notificationOccurred(.success) }
  shownStage = state.stage
  if state.complete { finishButton.setTitle("继续检查测量结果",for:.normal) }
  UIAccessibility.post(notification:.announcement,argument:state.title+"。"+state.instruction)
  // Speak only on a step change, never on every depth/tracking callback.
  // VoiceOver already provides the accessibility announcement.
  if !UIAccessibility.isVoiceOverRunning {
   speaker.stopSpeaking(at:.immediate)
   let utterance = AVSpeechUtterance(string:state.title+"。"+state.instruction)
   utterance.voice = AVSpeechSynthesisVoice(language:"zh-CN"); utterance.rate = 0.48
   speaker.speak(utterance)
  }
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
  super.viewDidDisappear(animated); speaker.stopSpeaking(at:.immediate); permissionCancelled = true; stopFrames()
  UIApplication.shared.isIdleTimerDisabled = false
  if !finished { reconstruction.cancel() }
 }
 private func stopFrames() {
  frameLock.lock(); acceptingFrames = false; frameLock.unlock(); camera.stop()
 }
 @objc private func finishScan() {
  guard !finished, !processing else { return }; processing = true; permissionCancelled = true; stopFrames()
  speaker.stopSpeaking(at:.immediate); finishButton.isEnabled = false; message.text = "正在 iPhone 上完成配准与重建，请保持应用在前台。"
  reconstruction.finish()
 }
 @objc private func cancelScan() { cancel() }
 private func stop(_ message: String) {
  guard !finished else { return }; permissionCancelled = true; reason = PersonalHrtfError.message(message)
  stopFrames(); finishButton.isEnabled = false; reconstruction.cancel()
 }
 private func end(_ result: Result<PersonalHeadMeshCandidate,Error>) {
  guard !finished else { return }; finished = true; permissionCancelled = true
  stopFrames(); speaker.stopSpeaking(at:.immediate); reconstruction.captureGuide = nil; reconstruction.progress = nil; reconstruction.captureLimit = nil; reconstruction.captureReadiness = nil; UIApplication.shared.isIdleTimerDisabled = false
  let answer: Result<PersonalHeadMeshCandidate,Error>
  if let reason {
   if case .success(let candidate) = result { try? FileManager.default.removeItem(at:candidate.directory) }
   answer = .failure(reason)
  } else { answer = result }
  let done = complete; complete = nil
  dismiss(animated:true) { done?(answer) }
 }
}
