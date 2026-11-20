import Foundation

struct PersonalHeadCaptureGuide: Equatable {
 enum EarSide { case left, right }
 enum Stage { case face, leftEar, rightEar, complete }
 private(set) var stage = Stage.face
 private(set) var frontCoverage: Float = 0
 var complete: Bool { stage == .complete }
 var targetEar: EarSide? { stage == .leftEar ? .left : (stage == .rightEar ? .right : nil) }
 var fraction: Float { complete ? 1 : (stage == .face ? frontCoverage : 0) }
 var title: String {
  switch stage { case .face: return "1 / 3  扫描正脸"; case .leftEar: return "2 / 3  扫描左耳"; case .rightEar: return "3 / 3  扫描右耳"; case .complete: return "正脸与双耳已识别" }
 }
 var symbol: String {
  switch stage { case .face: return "faceid"; case .leftEar: return "l.circle.fill"; case .rightEar: return "r.circle.fill"; case .complete: return "checkmark.circle.fill" }
 }
 var instruction: String {
  switch stage {
  case .face: return "手机保持在正前方，平视前置相机。"
  case .leftEar: return "手机不动，慢慢向自己的右边转头，让左耳朝向相机。"
  case .rightEar: return "左耳已记录。手机不动，经过正面慢慢向左转头，让右耳朝向相机。"
  case .complete: return "可以继续检查头耳的实测表面。"
  }
 }
 var status: String {
  let marks = [stage != .face,stage == .rightEar || complete,complete]
  return zip(["正脸","左耳","右耳"],marks).map { "\($0.0) \($0.1 ? "✓" : "○")" }.joined(separator:"    ")
 }
 mutating func show(front:Float, measuredEar:EarSide?) {
  frontCoverage = front
  switch stage {
  case .face where front >= 1: stage = .leftEar
  case .leftEar where measuredEar == .left: stage = .rightEar
  case .rightEar where measuredEar == .right: stage = .complete
  default: break
  }
 }
}
