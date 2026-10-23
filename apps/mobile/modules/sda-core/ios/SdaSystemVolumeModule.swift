import ExpoModulesCore
import AVFoundation
import MediaPlayer
import UIKit

// Keep Apple's slider geometry and touch area; center its public layout rect.
private final class CenteredSystemVolumeView: MPVolumeView {
 override func volumeSliderRect(forBounds bounds: CGRect) -> CGRect {
  var rect = super.volumeSliderRect(forBounds: bounds)
  rect.origin.y = bounds.midY - rect.height / 2
  return rect
 }
}

// MPVolumeView owns system-volume writes and route-specific behavior. The app
// observes changes only; mounting/reloading this view never sets device volume.
final class SdaSystemVolumeView: ExpoView, UIGestureRecognizerDelegate {
 private let volumeView = CenteredSystemVolumeView(frame: .zero)
 private let session = AVAudioSession.sharedInstance()
 private var observation: NSKeyValueObservation?
 private var observers: [NSObjectProtocol] = []
 private var lastValue: Float?
 let onChange = EventDispatcher()
 let onTracking = EventDispatcher()
 var activeColor = UIColor(white: 0.79, alpha: 1) { didSet { updateAppearance() } }
 var inactiveColor = UIColor(white: 1, alpha: 0.24) { didSet { updateAppearance() } }

 required init(appContext: AppContext? = nil) {
  super.init(appContext: appContext)
  backgroundColor = .clear
  volumeView.backgroundColor = .clear
  volumeView.showsRouteButton = false
  volumeView.showsVolumeSlider = true
  addSubview(volumeView)
  let tracking = UIPanGestureRecognizer(target:self, action:#selector(trackGesture(_:)))
  tracking.cancelsTouchesInView = false
  tracking.delaysTouchesBegan = false
  tracking.delegate = self
  volumeView.addGestureRecognizer(tracking)
  updateAppearance()
 }
 override func layoutSubviews() {
  super.layoutSubviews()
  volumeView.frame = bounds
 }
 override func didMoveToWindow() {
  super.didMoveToWindow()
  stopObserving()
  guard window != nil else { return }
  observation = session.observe(\.outputVolume, options:[.new]) { [weak self] _, _ in
   DispatchQueue.main.async { self?.reportVolume() }
  }
  for name in [AVAudioSession.routeChangeNotification, UIApplication.didBecomeActiveNotification] {
   observers.append(NotificationCenter.default.addObserver(forName:name,object:nil,queue:.main) { [weak self] _ in self?.reportVolume() })
  }
  DispatchQueue.main.async { [weak self] in self?.reportVolume() }
 }
 private func reportVolume() {
  guard window != nil else { return }
  let value = min(1, max(0, session.outputVolume))
  guard lastValue != value else { return }
  lastValue = value
  onChange(["value":Double(value)])
 }
 private func stopObserving() {
  observation?.invalidate(); observation = nil
  for observer in observers { NotificationCenter.default.removeObserver(observer) }
  observers.removeAll(); lastValue = nil
 }
 deinit { stopObserving() }
 @objc private func trackGesture(_ gesture: UIPanGestureRecognizer) {
  switch gesture.state {
  case .began: onTracking(["active":true])
  case .ended, .cancelled, .failed: onTracking(["active":false])
  default: break
  }
 }
 func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith otherGestureRecognizer: UIGestureRecognizer) -> Bool { true }
 private func trackImage(_ color: UIColor) -> UIImage {
  let image = UIGraphicsImageRenderer(size:CGSize(width:8,height:5)).image { _ in
   color.setFill(); UIBezierPath(roundedRect:CGRect(x:0,y:0,width:8,height:5),cornerRadius:2.5).fill()
  }
  return image.resizableImage(withCapInsets:UIEdgeInsets(top:2,left:3,bottom:2,right:3))
 }
 private func updateAppearance() {
  let thumb = UIGraphicsImageRenderer(size:CGSize(width:12,height:12)).image { _ in }
  for state: UIControl.State in [.normal, .highlighted, .disabled] {
   volumeView.setMinimumVolumeSliderImage(trackImage(activeColor),for:state)
   volumeView.setMaximumVolumeSliderImage(trackImage(inactiveColor),for:state)
   volumeView.setVolumeThumbImage(thumb,for:state)
  }
 }
 static func color(_ argb: Int) -> UIColor {
  let value = UInt32(truncatingIfNeeded:argb)
  return UIColor(red:CGFloat((value >> 16) & 255)/255,green:CGFloat((value >> 8) & 255)/255,blue:CGFloat(value & 255)/255,alpha:CGFloat(value >> 24)/255)
 }
}

public final class SdaSystemVolumeModule: Module {
 public func definition() -> ModuleDefinition {
  Name("SdaSystemVolume")
  View(SdaSystemVolumeView.self) {
   Events("onChange", "onTracking")
   Prop("activeColorArgb") { (view: SdaSystemVolumeView, value: Int) in view.activeColor = SdaSystemVolumeView.color(value) }
   Prop("inactiveColorArgb") { (view: SdaSystemVolumeView, value: Int) in view.inactiveColor = SdaSystemVolumeView.color(value) }
  }
 }
}
