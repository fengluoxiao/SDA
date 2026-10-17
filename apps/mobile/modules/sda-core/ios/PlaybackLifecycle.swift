import UIKit

/// A finite assertion for preparation before a renderer is producing audio.
/// Audio background mode, not this assertion, owns sustained playback.
final class PlaybackPreparationLease {
 private var identifier = UIBackgroundTaskIdentifier.invalid
 private var ended = false
 private let expired: () -> Void
 init(expired: @escaping () -> Void) {
  self.expired = expired
  DispatchQueue.main.async { [self] in
   guard !ended else { return }
   identifier = UIApplication.shared.beginBackgroundTask(withName:"SDA audio preparation") { [weak self] in
    guard let self else { return }
    self.endOnMain(); self.expired()
   }
  }
 }
 func end() { DispatchQueue.main.async { [self] in endOnMain() } }
 private func endOnMain() {
  ended = true
  if identifier != .invalid { UIApplication.shared.endBackgroundTask(identifier); identifier = .invalid }
 }
}
