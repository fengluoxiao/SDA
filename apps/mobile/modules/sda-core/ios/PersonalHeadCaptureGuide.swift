import Foundation

// Capture evidence only. This does not certify an acoustic mesh or locate an ear canal.
struct PersonalHeadCaptureGuide: Equatable {
 enum EarSide { case left, right }
 enum Stage: Int { case face, leftEar, rightEar, complete }
 private(set) var stage = Stage.face
 private(set) var head = 0
 private(set) var needsNewEarView = false
 private var emptyViewCount = 0
 private var oppositeVisible = false, subjectConfirmed = false
 private var previousSide: EarSide?
 private(set) var leftDepthSeen = false, rightDepthSeen = false
 private var lastFrameID: UInt64 = 0
 private var lastFaceTime = -Double.infinity, firstFaceTime = 0.0
 var complete: Bool { stage == .complete }
 var targetEar: EarSide? { stage == .leftEar ? .left : (stage == .rightEar ? .right : nil) }
 var canConfirmEar: Bool { targetEar != nil && (oppositeVisible || needsNewEarView) }
 var symbol: String {
  switch stage { case .face: return "faceid"; case .leftEar: return "l.circle.fill"; case .rightEar: return "r.circle.fill"; case .complete: return "checkmark.circle.fill" }
 }
 var fraction: Float { complete ? 1 : (stage == .face ? min(0.95,Float(head)/4) : 0) }
 var title: String {
  switch stage { case .face: return "1 / 3  扫描正脸"; case .leftEar: return "2 / 3  扫描左耳"; case .rightEar: return "3 / 3  扫描右耳"; case .complete: return "正脸与双耳已识别" }
 }
 var instruction: String {
  switch stage {
  case .face: return "将前置相机正对脸部，保持头部和表情不动，距离约 25–50 厘米。识别到正脸和有效深度后开始。"
  case .leftEar: return "请将前置相机移到你左手这一侧，对准左耳。"
  case .rightEar: return "左耳已记录。请将前置相机移到你右手这一侧，对准右耳。"
  case .complete: return "可以继续检查实测深度。无需扫描后脑。"
  }
 }
 var status: String {
  let marks = [stage != .face, stage == .rightEar || complete, complete]
  return zip(["正脸","左耳","右耳"],marks).map { "\($0.0) \($0.1 ? "✓" : "○")" }.joined(separator:"    ")
 }
 var reviewStatus: String {
  let missing = [("左耳",leftDepthSeen),("右耳",rightDepthSeen)].compactMap { $0.1 ? nil : $0.0 }
  return missing.isEmpty ? status : "双耳已识别；\(missing.joined(separator:"、"))识别时没有有效深度。"
 }
 mutating func observeFace(measured: Bool, time: Double, frameID: UInt64) {
  guard stage == .face, frameID > lastFrameID, time.isFinite, time > lastFaceTime else { return }
  lastFrameID = frameID
  if time-lastFaceTime > 2.5 { head = 0 }
  guard measured else { return }
  if head == 0 { firstFaceTime = time }
  lastFaceTime = time; head = min(4,head+1)
  if head == 4 && time-firstFaceTime >= 0.8 { stage = .leftEar }
 }
 mutating func observeEar(visible: Bool, hasDepth: Bool, frameID: UInt64, side: EarSide? = nil, singleEar: Bool = true) {
  guard let target = targetEar, frameID > lastFrameID else { return }
  lastFrameID = frameID
  oppositeVisible = false
  guard visible else {
   if needsNewEarView { emptyViewCount += 1; if emptyViewCount >= 2 { needsNewEarView = false } }
   return
  }
  emptyViewCount = 0
  if let side, side != target, !subjectConfirmed { oppositeVisible = true; return }
  if singleEar, side == .right, previousSide == .left { needsNewEarView = false }
  guard !needsNewEarView else { return }
  if stage == .leftEar { leftDepthSeen = hasDepth; previousSide = side; stage = .rightEar; needsNewEarView = true }
  else { rightDepthSeen = hasDepth; stage = .complete }
  subjectConfirmed = false
 }
 mutating func confirmEarView(_ target: EarSide, after frameID: UInt64) {
  guard targetEar == target, canConfirmEar else { return }
  lastFrameID = max(lastFrameID,frameID); needsNewEarView = false; oppositeVisible = false; subjectConfirmed = true
 }
}

// Sparse event join only: never stores camera buffers or substitutes a recent
// pose for a different frame. Call from one serial queue. Native frame dropping,
// inference delays and out-of-order callbacks cannot create false evidence.
struct PersonalHeadFrameJoin<Observation, Registration> {
 private struct Entry { var observation: Observation?; var registration: Registration? }
 private var entries: [UInt64: Entry] = [:]
 private var newest: UInt64 = 0, retiredThrough: UInt64 = 0
 let capacity: UInt64
 init(capacity: UInt64 = 64) { self.capacity = max(1,capacity) }
 var count: Int { entries.count }
 mutating func observe(_ observation: Observation, frameID: UInt64) -> (Observation, Registration)? {
  guard admit(frameID) else { return nil }
  var entry = entries[frameID] ?? Entry()
  guard entry.observation == nil else { return nil }
  entry.observation = observation; entries[frameID] = entry
  return take(frameID)
 }
 mutating func register(_ registration: Registration, frameID: UInt64) -> (Observation, Registration)? {
  guard admit(frameID) else { return nil }
  var entry = entries[frameID] ?? Entry()
  guard entry.registration == nil else { return nil }
  entry.registration = registration; entries[frameID] = entry
  return take(frameID)
 }
 private mutating func admit(_ frameID: UInt64) -> Bool {
  guard frameID > retiredThrough else { return false }
  newest = max(newest,frameID)
  let floor = newest > capacity ? newest-capacity : 0
  entries = entries.filter { $0.key > floor && $0.key > retiredThrough }
  return frameID > floor
 }
 private mutating func take(_ frameID: UInt64) -> (Observation, Registration)? {
  guard let entry = entries[frameID], let observation = entry.observation,
        let registration = entry.registration else { return nil }
  retiredThrough = frameID
  entries = entries.filter { $0.key > retiredThrough }
  return (observation,registration)
 }
}
