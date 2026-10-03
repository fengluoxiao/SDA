import ExpoModulesCore
import UIKit

// Diagnostics are opt-in for CI only; no timers, file writes or synthetic input
// in a user's app. All rendering and control events stay on the main thread.
private enum ChromeSmoke {
 static var entries: [String: [String: Any]] = [:]
 // New SDK UI is owned by SwiftUI/UIKit, not the old SDA wrapper views.
 // Inspect real mounted native views in opt-in CI instead of inventing records
 // for controls that are no longer used. No timers run in normal playback.
 static var inspecting = false
 static func startInspection() {
  guard !inspecting,
   ProcessInfo.processInfo.environment["SDA_IOS_SCENE_SMOKE"] == "1" || ProcessInfo.processInfo.environment["SDA_IOS_CHROME_SMOKE"] != nil else { return }
  inspecting = true
  DispatchQueue.main.async {
   var remaining = 45
   Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { timer in
    remaining -= 1
    var views: [[String: Any]] = []
    var navigationBars: [[String: Any]] = []
    func visit(_ view: UIView) {
     // UIWindow.window can be nil: still traverse its mounted descendants.
     // Test visibility only when recording, never cut off the window subtree.
     if view.window != nil && !view.isHidden && view.alpha > 0.01 && view.bounds.width > 0 && view.bounds.height > 0 {
      let text = (view as? UILabel)?.text ?? ""
      views.append(["class": String(describing: type(of: view)), "label": view.accessibilityLabel ?? "", "text": text, "alpha": view.alpha, "width": view.bounds.width, "height": view.bounds.height])
      if let bar = view as? UINavigationBar {
       let appearance = bar.topItem?.standardAppearance ?? bar.standardAppearance
       let edge = bar.topItem?.scrollEdgeAppearance ?? bar.scrollEdgeAppearance ?? appearance
       navigationBars.append(["title":bar.topItem?.title ?? "", "backgroundBlur":appearance.backgroundEffect != nil,
        "scrollEdgeBlur":edge.backgroundEffect != nil, "height":bar.bounds.height])
      }
     }
     view.subviews.forEach(visit)
    }
    for scene in UIApplication.shared.connectedScenes {
     (scene as? UIWindowScene)?.windows.filter { !$0.isHidden }.forEach(visit)
    }
    let report: [String: Any] = ["ok": true, "ios": UIDevice.current.systemVersion, "controls": entries, "nativeViews": views, "navigationBars": navigationBars]
    if let data = try? JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]) {
     let directory = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
     try? data.write(to: directory.appendingPathComponent("sda-ci-chrome.json"), options: .atomic)
    }
    if remaining <= 0 { timer.invalidate() }
   }
  }
 }
 static func record(_ key: String, kind: String, view: UIView) {
  guard ProcessInfo.processInfo.environment["SDA_IOS_SCENE_SMOKE"] == "1" || ProcessInfo.processInfo.environment["SDA_IOS_CHROME_SMOKE"] != nil,
        !key.isEmpty, view.window != nil, view.bounds.width > 0 else { return }
  let entry: [String: Any] = ["material": kind, "width": view.bounds.width, "height": view.bounds.height]
  if let previous = entries[key], NSDictionary(dictionary: previous).isEqual(to: entry) { return }
  entries[key] = entry
  let report: [String: Any] = ["ok": true, "ios": UIDevice.current.systemVersion, "controls": entries]
  if let data = try? JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]) {
   let directory = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
   try? data.write(to: directory.appendingPathComponent("sda-ci-chrome.json"), options: .atomic)
  }
 }
}

final class SdaGlassButtonView: ExpoView {
 let control = GlassButtonControl(frame: .zero)
 let onPress = EventDispatcher()
 let onChoice = EventDispatcher()
 required init(appContext: AppContext? = nil) {
  super.init(appContext: appContext)
  addSubview(control)
  control.action = { [weak self] in self?.onPress([:]) }
  control.choose = { [weak self] index in self?.onChoice(["index": index]) }
 }
 override func layoutSubviews() {
  super.layoutSubviews(); control.frame = bounds; control.layoutIfNeeded()
  ChromeSmoke.record(control.label, kind: control.materialKind, view: self)
 }
}
public final class SdaGlassButtonModule: Module {
 public func definition() -> ModuleDefinition {
  Name("SdaGlassButton")
  Function("smokeStage") { () -> String in
   ChromeSmoke.startInspection()
   return ProcessInfo.processInfo.environment["SDA_IOS_CHROME_SMOKE"] ?? ""
  }
  View(SdaGlassButtonView.self) {
   Events("onPress", "onChoice")
   Prop("title") { (view: SdaGlassButtonView, value: String) in view.control.title = value }
   Prop("subtitle") { (view: SdaGlassButtonView, value: String) in view.control.subtitle = value }
   Prop("row") { (view: SdaGlassButtonView, value: Bool) in view.control.row = value }
   Prop("selected") { (view: SdaGlassButtonView, value: Bool) in view.control.selected = value }
   Prop("choices") { (view: SdaGlassButtonView, value: [String]) in view.control.choices = value }
   Prop("choiceIndex") { (view: SdaGlassButtonView, value: Int) in view.control.choiceIndex = value }
   Prop("symbol") { (view: SdaGlassButtonView, value: String) in view.control.symbol = value }
   Prop("label") { (view: SdaGlassButtonView, value: String) in view.control.label = value; view.setNeedsLayout() }
   Prop("enabled") { (view: SdaGlassButtonView, value: Bool) in view.control.enabled = value }
   Prop("prominent") { (view: SdaGlassButtonView, value: Bool) in view.control.prominent = value }
   Prop("symbolSize") { (view: SdaGlassButtonView, value: Double) in view.control.symbolSize = CGFloat(value) }
  }
 }
}

final class SdaGlassTabsView: ExpoView {
 let control = GlassTabsControl(frame: .zero)
 let onChange = EventDispatcher()
 required init(appContext: AppContext? = nil) {
  super.init(appContext: appContext); addSubview(control)
  control.action = { [weak self] index in self?.onChange(["index": index]) }
 }
 override func layoutSubviews() {
  super.layoutSubviews(); control.frame = bounds; control.layoutIfNeeded()
  ChromeSmoke.record("tabs", kind: control.materialKind, view: self)
 }
}
public final class SdaGlassTabsModule: Module {
 public func definition() -> ModuleDefinition {
  Name("SdaGlassTabs")
  View(SdaGlassTabsView.self) {
   Events("onChange")
   Prop("selected") { (view: SdaGlassTabsView, value: Int) in view.control.selected = min(2, max(0, value)) }
  }
 }
}

final class SdaMaterialSurfaceView: ExpoView {
 let control = MaterialSurfaceControl(frame: .zero)
 required init(appContext: AppContext? = nil) { super.init(appContext: appContext); addSubview(control) }
 override func layoutSubviews() {
  super.layoutSubviews(); control.frame = bounds
  ChromeSmoke.record("settingsSurface", kind: UIAccessibility.isReduceTransparencyEnabled ? "reducedTransparency" : "systemMaterial", view: self)
 }
}
public final class SdaMaterialSurfaceModule: Module {
 public func definition() -> ModuleDefinition {
  Name("SdaMaterialSurface")
  View(SdaMaterialSurfaceView.self) {}
 }
}

final class SdaNativeSliderView: ExpoView {
 let control = NativeSliderControl(frame: .zero)
 let onChange = EventDispatcher()
 let onTracking = EventDispatcher()
 required init(appContext: AppContext? = nil) {
  super.init(appContext: appContext); addSubview(control)
  control.changed = { [weak self] value in self?.onChange(["value": value]) }
  control.tracking = { [weak self] active in self?.onTracking(["active": active]) }
 }
 override func layoutSubviews() {
  super.layoutSubviews(); control.frame = bounds; control.layoutIfNeeded()
  ChromeSmoke.record("volumeSlider", kind: "UISlider", view: self)
 }
}
public final class SdaNativeSliderModule: Module {
 public func definition() -> ModuleDefinition {
  Name("SdaNativeSlider")
  View(SdaNativeSliderView.self) {
   Events("onChange", "onTracking")
   Prop("value") { (view: SdaNativeSliderView, value: Double) in view.control.value = min(1, max(0, value)) }
  }
 }
}
final class SdaNativeStepperView: ExpoView {
 let control = NativeStepperControl(frame: .zero)
 let onChange = EventDispatcher()
 required init(appContext: AppContext? = nil) {
  super.init(appContext: appContext); addSubview(control)
  control.changed = { [weak self] value in self?.onChange(["value": value]) }
 }
 override func layoutSubviews() {
  super.layoutSubviews(); control.frame = bounds; control.layoutIfNeeded()
  ChromeSmoke.record("distanceStepper", kind: "UIStepper", view: self)
 }
}
public final class SdaNativeStepperModule: Module {
 public func definition() -> ModuleDefinition {
  Name("SdaNativeStepper")
  View(SdaNativeStepperView.self) {
   Events("onChange")
   Prop("value") { (view: SdaNativeStepperView, value: Double) in view.control.stepper.value = min(4, max(0.25, value)) }
   Prop("enabled") { (view: SdaNativeStepperView, value: Bool) in view.control.stepper.isEnabled = value }
  }
 }
}
