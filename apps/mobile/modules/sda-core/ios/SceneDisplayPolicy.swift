import UIKit

/// Display-only requests; never changes audio scheduling or object metadata.
enum SceneDisplayPolicy {
  static let key = "sda.scene.preferredFrameRate"
  static func capabilities() -> [String: Any] {
    let read = { () -> [String: Any] in
      let screen = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first(where: { $0.activationState == .foregroundActive })?.screen ?? UIScreen.main
      return ["maximumFps": screen.maximumFramesPerSecond]
    }
    return Thread.isMainThread ? read() : DispatchQueue.main.sync(execute: read)
  }
  static func configure(active: Bool, fps: Int) {
    DispatchQueue.main.async {
      let maximum = capabilities()["maximumFps"] as? Int ?? 60
      UserDefaults.standard.set(active ? (fps == 120 && maximum >= 120 ? 120 : 60) : 0, forKey: key)
    }
  }
}
