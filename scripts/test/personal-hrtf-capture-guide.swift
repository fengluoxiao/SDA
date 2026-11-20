import Foundation

enum Failure: Error { case invalid(String) }
@main struct CaptureGuideRegression {
 static func main() throws {
  var guide = PersonalHeadCaptureGuide()
  guide.observeEar(visible:true,hasDepth:true,frameID:1)
  guide.observeFace(measured:false,time:1,frameID:1)
  for frameID in UInt64(2)...4 { guide.observeFace(measured:true,time:Double(frameID)/3,frameID:frameID) }
  guide.observeFace(measured:true,time:2,frameID:4)
  guard guide.stage == .face, guide.head == 3 else { throw Failure.invalid("Unmeasured or duplicate frame advanced face setup") }
  guide.observeFace(measured:true,time:5,frameID:5)
  guard guide.head == 1 else { throw Failure.invalid("A gap accumulated face measurements") }
  for frameID in UInt64(6)...8 { guide.observeFace(measured:true,time:Double(frameID),frameID:frameID) }
  guide.observeEar(visible:true,hasDepth:false,frameID:8)
  guide.observeEar(visible:false,hasDepth:true,frameID:9)
  guard guide.stage == .leftEar else { throw Failure.invalid("A face frame or blank image was counted as an ear") }
  var sided = guide
  guard sided.symbol == "l.circle.fill", sided.targetEar == .left else { throw Failure.invalid("Subject left cue reversed") }
  sided.observeEar(visible:true,hasDepth:true,frameID:10,side:.right)
  guard sided.stage == .leftEar, sided.canConfirmEar else { throw Failure.invalid("An identified right ear was recorded as left") }
  sided.observeEar(visible:true,hasDepth:false,frameID:11,side:.left)
  guard sided.stage == .rightEar, sided.symbol == "r.circle.fill", !sided.leftDepthSeen else { throw Failure.invalid("Subject left evidence or right cue reversed") }
  sided.observeEar(visible:true,hasDepth:true,frameID:12,side:.left)
  guard !sided.complete else { throw Failure.invalid("An identified left ear was recorded as right") }
  sided.observeEar(visible:true,hasDepth:true,frameID:13,side:.right)
  guard sided.complete, sided.rightDepthSeen else { throw Failure.invalid("Opposite anatomical ear required an empty frame") }
  guide.observeEar(visible:true,hasDepth:false,frameID:10)
  guard guide.stage == .rightEar, guide.needsNewEarView, !guide.leftDepthSeen else { throw Failure.invalid("First visible ear required depth or a second frame") }
  for frameID in UInt64(11)...120 { guide.observeEar(visible:true,hasDepth:true,frameID:frameID) }
  guide.observeEar(visible:false,hasDepth:false,frameID:121)
  guide.observeEar(visible:true,hasDepth:true,frameID:122)
  guard !guide.complete, guide.needsNewEarView else { throw Failure.invalid("Continuous visibility or one missed inference counted the first ear twice") }
  var manual = guide
  manual.confirmEarView(.right,after:130)
  manual.observeEar(visible:true,hasDepth:false,frameID:130)
  manual.observeEar(visible:false,hasDepth:false,frameID:131)
  guard !manual.complete else { throw Failure.invalid("Manual handoff counted an old frame or a blank view") }
  manual.observeEar(visible:true,hasDepth:false,frameID:132)
  guard manual.complete, !manual.rightDepthSeen, manual.reviewStatus.contains("左耳、右耳") else { throw Failure.invalid("Right ear required geometry or fabricated depth") }
  guide.observeEar(visible:false,hasDepth:false,frameID:123)
  guide.observeEar(visible:false,hasDepth:false,frameID:124)
  guide.observeEar(visible:true,hasDepth:true,frameID:125)
  guard guide.complete, guide.rightDepthSeen, !guide.leftDepthSeen else { throw Failure.invalid("First detection after automatic handoff did not complete") }

  var join = PersonalHeadFrameJoin<String,Bool>(capacity:4)
  guard join.observe("one",frameID:1) == nil, join.register(true,frameID:2) == nil,
        let first = join.register(false,frameID:1), first.0 == "one", !first.1,
        join.observe("duplicate",frameID:1) == nil,
        let second = join.observe("two",frameID:2), second.0 == "two", second.1 else { throw Failure.invalid("Face pose paired with the wrong observation") }
  guard join.observe("dropped",frameID:3) == nil, join.register(true,frameID:4) == nil,
        let fourth = join.observe("four",frameID:4), fourth.0 == "four",
        join.register(true,frameID:3) == nil else { throw Failure.invalid("Reused stale face pose") }
  for frameID in UInt64(5)...10000 {
   _ = join.register(true,frameID:frameID)
   guard join.count <= 4 else { throw Failure.invalid("Unbounded frame join") }
  }
  guard join.observe("expired",frameID:9996) == nil, join.observe("last",frameID:10000)?.1 == true,
        join.observe("zero",frameID:0) == nil else { throw Failure.invalid("Incorrect registration eviction") }
  print("{\"singleDetectionPerEar\":true,\"repeatedViewNotDoubleCounted\":true,\"automaticAndManualHandoff\":true,\"missingDepthNotFabricated\":true,\"exactFrameJoinPassed\":true,\"sparseJoinBounded\":true,\"physicalScanTested\":false}")
 }
}
