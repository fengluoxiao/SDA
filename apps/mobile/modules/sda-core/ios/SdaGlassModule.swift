import ExpoModulesCore
import UIKit

// Diagnostics are opt-in for CI only; no timers, file writes or synthetic input
// in a user's app. All rendering and control events stay on the main thread.
private enum ChromeSmoke {
 static var entries: [String: [String: Any]] = [:]
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
 required init(appContext: AppContext? = nil) {
  super.init(appContext: appContext)
  addSubview(control)
  control.action = { [weak self] in self?.onPress([:]) }
 }
 override func layoutSubviews() {
  super.layoutSubviews(); control.frame = bounds; control.layoutIfNeeded()
  ChromeSmoke.record(control.label, kind: control.materialKind, view: self)
 }
}
public final class SdaGlassButtonModule: Module {
 public func definition() -> ModuleDefinition {
  Name("SdaGlassButton")
  Function("smokeStage") { ProcessInfo.processInfo.environment["SDA_IOS_CHROME_SMOKE"] ?? "" }
  View(SdaGlassButtonView.self) {
   Events("onPress")
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
