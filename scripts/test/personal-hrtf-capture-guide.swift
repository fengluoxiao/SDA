import Foundation
import simd

enum Failure: Error { case invalid(String) }
@main struct CaptureGuideRegression {
 static func main() throws {
  var state = PersonalHeadCaptureGuide()
  let leftEarPoint = SIMD3<Float>(0.08,0,0), rightEarPoint = SIMD3<Float>(-0.08,0,0)
  var clock = 1.0
  func observe(_ head:Bool = false,_ left:SIMD3<Float>? = nil,_ right:SIMD3<Float>? = nil,_ camera:SIMD3<Float> = .zero) {
   state.observe(head:head,left:left,right:right,camera:camera,time:clock); clock += 0.31
  }
  for _ in 0..<10 { observe(false,leftEarPoint,rightEarPoint) }
  guard state.stage == .face, state.head == 0, state.leftEar == 0, state.rightEar == 0 else { throw Failure.invalid("Ears skipped the required face setup") }
  for _ in 0..<4 { observe(true,leftEarPoint,rightEarPoint) }
  guard state.stage == .leftEar, state.head == 4, state.leftEar == 0 else { throw Failure.invalid("Face did not advance alone") }
  for _ in 0..<10 { observe(true,nil,rightEarPoint) }
  guard state.stage == .leftEar, state.leftEar == 0, state.rightEar == 0 else { throw Failure.invalid("Wrong ear advanced the current step") }
  for _ in 0..<10 { observe(false,leftEarPoint) }
  guard state.stage == .leftEar, !state.complete else { throw Failure.invalid("One frozen ear view qualified multi-view capture") }
  observe(false,SIMD3<Float>(0.15,0,0))
  guard state.leftEar == 1 else { throw Failure.invalid("Unstable ear evidence accumulated") }
  // A skipped ML frame at ~2 Hz must not repeatedly zero the ear step.
  observe(false,leftEarPoint,nil,SIMD3<Float>(0.001,0,0))
  clock += 1.5
  observe(false,leftEarPoint,nil,SIMD3<Float>(0.002,0,0))
  guard state.leftEar >= 2 else { throw Failure.invalid("Ordinary skipped inference reset the ear") }
  for i in 0..<7 { observe(false,leftEarPoint,nil,SIMD3<Float>(Float(i)*0.002,0,0)) }
  guard state.stage == .rightEar, state.leftEar == 6 else { throw Failure.invalid("Measured left-ear views did not advance") }
  for _ in 0..<20 { observe(true,leftEarPoint,nil) }
  guard state.stage == .rightEar, state.head == 4, state.leftEar == 6, state.rightEar == 0 else { throw Failure.invalid("Journey through the face reset a completed step") }
  observe(false,nil,rightEarPoint)
  state.observe(head:false,left:nil,right:rightEarPoint,camera:SIMD3<Float>(0.1,0,0),time:clock-0.31)
  guard state.rightEar == 1 else { throw Failure.invalid("Duplicate callback counted as evidence") }
  for _ in 0..<10 { observe() }
  guard state.rightEar == 0 else { throw Failure.invalid("Occlusion did not clear incomplete step evidence") }
  for i in 0..<7 { observe(false,nil,rightEarPoint,SIMD3<Float>(Float(i)*0.002,0,0)) }
  guard state.complete, state.head == 4, state.leftEar == 6, state.rightEar == 6 else { throw Failure.invalid("Valid sequential capture failed to complete") }
  for _ in 0..<2000 { observe() }
  guard state.complete else { throw Failure.invalid("Completed capture was lost") }
  let center = SIMD3<Float>.zero, axis = SIMD3<Float>(1,0,0)
  func side(_ point:SIMD3<Float>,_ camera:SIMD3<Float>) -> Int {
   PersonalHeadCaptureGuide.qualifiedEarSide(point:point,center:center,subjectLeft:axis,camera:camera)
  }
  guard side(leftEarPoint,SIMD3<Float>(0,0,0.3)) == 0,
        side(rightEarPoint,SIMD3<Float>(0,0,0.3)) == 0 else { throw Failure.invalid("Frontal ear boxes qualified side capture") }
  guard side(leftEarPoint,SIMD3<Float>(0.25,0,0.25)) == 1,
        side(rightEarPoint,SIMD3<Float>(-0.25,0,0.25)) == -1 else { throw Failure.invalid("Anatomical side views were rejected") }
  guard side(leftEarPoint,SIMD3<Float>(-0.25,0,0.25)) == 0,
        side(rightEarPoint,SIMD3<Float>(0.25,0,0.25)) == 0 else { throw Failure.invalid("Opposite camera side qualified the ear") }
  guard side(SIMD3<Float>(0.01,0,0),SIMD3<Float>(0.25,0,0.25)) == 0,
        side(SIMD3<Float>(0.3,0,0),SIMD3<Float>(0.25,0,0.25)) == 0,
        side(SIMD3<Float>(.nan,0,0),SIMD3<Float>(0.25,0,0.25)) == 0,
        side(leftEarPoint,SIMD3<Float>(0.1,0,0)) == 0 else { throw Failure.invalid("Invalid or unrelated measurements qualified the ear") }
  for degrees in stride(from:0,through:90,by:5) {
   let angle = Float(degrees)*Float.pi/180
   for sign:Float in [-1,1] {
    let camera = SIMD3<Float>(sign*sin(angle)*0.3,0,cos(angle)*0.3)
    let point = SIMD3<Float>(sign*0.08,0,0)
    let expected = sin(angle) >= 0.2 ? Int(sign) : 0
    guard side(point,camera) == expected else { throw Failure.invalid("Viewpoint angle or side symmetry failed") }
   }
  }
  // Pair by source identity, never by arrival time or the latest pose.
  var join = PersonalHeadFrameJoin<String,Bool>(capacity:4)
  guard join.observe("ear-1",frameID:1) == nil, join.register(true,frameID:2) == nil,
        let first = join.register(false,frameID:1), first.0 == "ear-1", first.1 == false,
        join.observe("duplicate",frameID:1) == nil,
        let second = join.observe("ear-2",frameID:2), second.0 == "ear-2", second.1 == true else {
   throw Failure.invalid("Source identity lost across reordered/rejected/duplicate registrations")
  }
  // A dropped native input must not consume a different frame's measurement.
  guard join.observe("dropped",frameID:3) == nil, join.register(true,frameID:4) == nil,
        let fourth = join.observe("ear-4",frameID:4), fourth.0 == "ear-4",
        join.register(true,frameID:3) == nil else { throw Failure.invalid("Dropped or stale frame reused a pose") }
  for id in UInt64(5)...10000 {
   _ = join.register(id.isMultiple(of:2),frameID:id)
   guard join.count <= 4 else { throw Failure.invalid("Sparse join exceeded its fixed capacity") }
  }
  guard join.observe("expired",frameID:9996) == nil,
        let last = join.observe("ear-10000",frameID:10000), last.1,
        join.observe("zero",frameID:0) == nil else { throw Failure.invalid("Evicted or zero frame ID was revived") }
  print("{\"sequentialCapturePassed\":true,\"wrongTargetRejected\":true,\"duplicateRejected\":true,\"occlusionAndCompletedStatePassed\":true,\"earMultiViewRequired\":true,\"frontalEarViewsRejected\":true,\"anatomicalViewpointAngleSweepPassed\":true,\"exactFrameJoinPassed\":true,\"sparseJoinBounded\":true,\"physicalScanTested\":false}")
 }
}
