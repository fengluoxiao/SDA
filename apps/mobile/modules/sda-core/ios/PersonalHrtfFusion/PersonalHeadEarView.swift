import simd

// FaceFrame's two yaw bins tell us that it fused a side view. Name the ear
// using ARKit's labelled, front-calibrated eyes in that same fused head frame.
enum PersonalHeadEarView {
 static func side(advanced: Bool, pose: simd_float4x4?, eyes: (left: Vec3, right: Vec3)?) -> PersonalHeadCaptureGuide.EarSide? {
  guard advanced, let pose, let eyes else { return nil }
  // Adapt the ordering observed on the front TrueDepth capture path once,
  // before either coverage or support gets an anatomical label. In 967 the
  // positive projection onto (left slot - right slot) credited the user's
  // physical right ear as left. Do not mirror the RGB preview or mesh here.
  let leftward = simd_normalize(eyes.right - eyes.left)
  let toCamera = simd_normalize(pose.translation - (eyes.left + eyes.right) / 2)
  // Camera +Z points back toward the phone. Translation changes the first
  // projection, not the second; rolling the phone changes neither.
  let nearEye = simd_dot(toCamera, leftward)
  let heading = simd_dot(simd_normalize(pose.transformDirection(Vec3(0, 0, 1))), leftward)
  // FaceFrame already requires a >55-degree side view; allow an off-axis
  // phone, but not a frontal head merely shifted to one side of the image.
  if nearEye > 0.5 && heading > 0.5 { return .left }
  if nearEye < -0.5 && heading < -0.5 { return .right }
  return nil
 }
}
