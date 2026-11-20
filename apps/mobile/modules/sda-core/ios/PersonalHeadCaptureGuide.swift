import Foundation
import simd

// Capture evidence only. This does not certify an acoustic mesh or locate an ear canal.
struct PersonalHeadCaptureGuide: Equatable {
 enum Stage: Int { case face, leftEar, rightEar, complete }
 private(set) var stage = Stage.face
 private(set) var head = 0, leftEar = 0, rightEar = 0
 private var lastTime = -Double.infinity, lastEvidenceTime = -Double.infinity, firstTime = 0.0
 private var lastPoint: SIMD3<Float>?, firstCamera: SIMD3<Float>?
 private var baseline: Float = 0
 var complete: Bool { stage == .complete }
 var samples: Int { stage == .face ? head : (stage == .leftEar ? leftEar : (stage == .rightEar ? rightEar : 6)) }
 var required: Int { stage == .face ? 4 : 6 }
 var fraction: Float { complete ? 1 : min(0.95,Float(samples)/Float(required)) }
 var title: String {
  switch stage { case .face: return "1 / 3  扫描正脸"; case .leftEar: return "2 / 3  扫描左耳"; case .rightEar: return "3 / 3  扫描右耳"; case .complete: return "三个部位已采集" }
 }
 var instruction: String {
  switch stage {
  case .face: return "将前置相机正对脸部，保持头部和表情不动，距离约 25–50 厘米。识别到正脸和有效深度后开始。"
  case .leftEar: return "保持头部不动，将手机从正面沿脸侧缓慢移向本人的左耳。露出耳廓，保留一点脸侧，在耳旁小幅移动手机。"
  case .rightEar: return "左耳已保留。将手机沿原路缓慢回到正面，再移向本人的右耳；露出耳廓，在耳旁小幅移动手机。"
  case .complete: return "正脸、左耳、右耳已依次记录，可以继续检查测量结果。无需扫描后脑。"
  }
 }
 var status: String {
  let marks = [head >= 4, leftEar >= 6, rightEar >= 6]
  return zip(["正脸","左耳","右耳"],marks).map { "\($0.0) \($0.1 ? "✓" : "○")" }.joined(separator:"    ")
 }
 // A wrong/missing target never advances this step. Completed steps survive the
 // journey across the face. Duplicate/stale callbacks cannot satisfy evidence.
 mutating func observe(head seenHead: Bool, left: SIMD3<Float>?, right: SIMD3<Float>?, camera: SIMD3<Float>, time: Double) {
  guard !complete, time.isFinite, time > lastTime,
        camera.x.isFinite, camera.y.isFinite, camera.z.isFinite else { return }
  let point = stage == .face ? (seenHead ? SIMD3<Float>.zero : nil) : (stage == .leftEar ? left : right)
  let gap = time-lastEvidenceTime; lastTime = time
  guard let point, point.x.isFinite, point.y.isFinite, point.z.isFinite else {
   if gap > 1.2 { resetCurrent() }
   return
  }
  if gap > 1.2 || (lastPoint.map { simd_distance($0,point) > 0.025 } ?? false) { resetCurrent() }
  if firstCamera == nil { firstCamera = camera; firstTime = time }
  lastEvidenceTime = time; lastPoint = point
  baseline = max(baseline,simd_distance(firstCamera!,camera))
  switch stage { case .face: head = min(4,head+1); case .leftEar: leftEar = min(5,leftEar+1); case .rightEar: rightEar = min(5,rightEar+1); case .complete: break }
  let ready = stage == .face ? head >= 4 && time-firstTime >= 0.8 : samples >= 5 && time-firstTime >= 1.2 && baseline >= 0.008
  if ready {
   if stage == .leftEar { leftEar = 6 }; if stage == .rightEar { rightEar = 6 }
   stage = Stage(rawValue:stage.rawValue+1)!
   lastPoint = nil; firstCamera = nil; baseline = 0; firstTime = 0; lastEvidenceTime = -Double.infinity
  }
 }
 private mutating func resetCurrent() {
  switch stage { case .face: head = 0; case .leftEar: leftEar = 0; case .rightEar: rightEar = 0; case .complete: break }
  lastPoint = nil; firstCamera = nil; baseline = 0; firstTime = 0
 }
}
