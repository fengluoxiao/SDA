import UIKit
import SceneKit
import simd

// Review the actual measured model without changing player UI, audio session,
// playback, orientation or selected HRTF. No generated anatomy is displayed.
final class PersonalHrtfReviewController: UIViewController {
 private let surface: PersonalHrtfReviewGeometry
 private let sceneView = SCNView(), model = SCNNode(), selection = SCNNode()
 private let step = UISegmentedControl(items:["左耳","右耳","鼻尖","头顶"])
 private let message = UILabel(), advanceButton = UIButton(type:.system)
 private var left = Set<UInt32>(), right = Set<UInt32>()
 private var nose: PersonalHrtfReviewGeometry.Landmark?, crown: PersonalHrtfReviewGeometry.Landmark?
 private var ended = false
 var complete: ((Result<PersonalHrtfReviewGeometry.Confirmation,Error>) -> Void)?
 init(surface: PersonalHrtfReviewGeometry) { self.surface = surface; super.init(nibName:nil,bundle:nil) }
 required init?(coder:NSCoder) { fatalError("Use measured surface") }
 override func viewDidLoad() {
  super.viewDidLoad(); view.backgroundColor = .systemBackground; isModalInPresentation = true
  sceneView.translatesAutoresizingMaskIntoConstraints = false; sceneView.backgroundColor = .secondarySystemBackground
  sceneView.allowsCameraControl = true; sceneView.autoenablesDefaultLighting = true
  sceneView.isPlaying = false; sceneView.rendersContinuously = false; sceneView.preferredFramesPerSecond = 30
  sceneView.scene = SCNScene(); view.addSubview(sceneView)
  let source = SCNGeometrySource(vertices:stride(from:0,to:surface.vertices.count,by:3).map { SCNVector3(surface.vertices[$0],surface.vertices[$0+1],surface.vertices[$0+2]) })
  // Normals are derived solely for shading; measured vertices and face IDs
  // remain unchanged for hit testing and acoustic inputs.
  var normals = [SIMD3<Double>](repeating:.zero,count:surface.vertices.count/3)
  for face in 0..<surface.triangles.count/3 {
   let ids = Array(surface.triangles[3*face..<3*face+3])
   let a = surface.vertex(ids[0]), b = surface.vertex(ids[1]), c = surface.vertex(ids[2])
   let n = simd_cross(b-a,c-a)
   for id in ids { normals[Int(id)] += n }
  }
  let normalSource = SCNGeometrySource(normals:normals.map { n in
   let unit = simd_length(n) > 1e-12 ? simd_normalize(n) : SIMD3<Double>(0,0,1)
   return SCNVector3(Float(unit.x),Float(unit.y),Float(unit.z))
  })
  let element = SCNGeometryElement(data:surface.triangles.withUnsafeBufferPointer { Data(buffer:$0) },primitiveType:.triangles,primitiveCount:surface.triangles.count/3,bytesPerIndex:4)
  model.geometry = SCNGeometry(sources:[source,normalSource],elements:[element])
  let material = SCNMaterial(); material.diffuse.contents = UIColor.systemGray; material.isDoubleSided = true
  model.geometry?.materials = [material]; sceneView.scene?.rootNode.addChildNode(model)
  sceneView.scene?.rootNode.addChildNode(selection)
  let (low,high) = model.boundingBox, center = SCNVector3((low.x+high.x)/2,(low.y+high.y)/2,(low.z+high.z)/2)
  let extent = max(high.x-low.x,max(high.y-low.y,high.z-low.z))
  let camera = SCNNode(); camera.camera = SCNCamera(); camera.camera?.zNear = 0.001; camera.camera?.zFar = Double(max(5,extent*10))
  camera.position = SCNVector3(center.x,center.y,center.z+max(0.1,extent*2)); camera.look(at:center)
  sceneView.scene?.rootNode.addChildNode(camera); sceneView.pointOfView = camera
  sceneView.defaultCameraController.target = center
  sceneView.addGestureRecognizer(UITapGestureRecognizer(target:self,action:#selector(selectSurface(_:))))
  step.selectedSegmentIndex = 0; step.addTarget(self,action:#selector(changeStep),for:.valueChanged)
  message.numberOfLines = 0; message.font = .preferredFont(forTextStyle:.body); message.adjustsFontForContentSizeCategory = true; message.textColor = .label
  advanceButton.setTitle("下一步",for:.normal); advanceButton.addTarget(self,action:#selector(advance),for:.touchUpInside)
  let reset = UIButton(type:.system); reset.setTitle("清除此项",for:.normal); reset.addTarget(self,action:#selector(clearStep),for:.touchUpInside)
  let cancel = UIButton(type:.system); cancel.setTitle("取消并删除采集",for:.normal); cancel.addTarget(self,action:#selector(cancelReview),for:.touchUpInside)
  let buttons = UIStackView(arrangedSubviews:[reset,advanceButton]); buttons.distribution = .fillEqually; buttons.spacing = 12
  let controls = UIStackView(arrangedSubviews:[step,message,buttons,cancel]); controls.axis = .vertical; controls.spacing = 10
  // Keep confirm and cancel reachable in landscape and at large text sizes.
  let scroll = UIScrollView(); scroll.translatesAutoresizingMaskIntoConstraints = false; view.addSubview(scroll)
  controls.translatesAutoresizingMaskIntoConstraints = false; scroll.addSubview(controls)
  NSLayoutConstraint.activate([
   sceneView.topAnchor.constraint(equalTo:view.safeAreaLayoutGuide.topAnchor),sceneView.leadingAnchor.constraint(equalTo:view.safeAreaLayoutGuide.leadingAnchor),sceneView.trailingAnchor.constraint(equalTo:view.safeAreaLayoutGuide.trailingAnchor),
   sceneView.heightAnchor.constraint(equalTo:view.safeAreaLayoutGuide.heightAnchor,multiplier:0.55),
   scroll.topAnchor.constraint(equalTo:sceneView.bottomAnchor,constant:10),scroll.leadingAnchor.constraint(equalTo:view.safeAreaLayoutGuide.leadingAnchor),scroll.trailingAnchor.constraint(equalTo:view.safeAreaLayoutGuide.trailingAnchor),scroll.bottomAnchor.constraint(equalTo:view.safeAreaLayoutGuide.bottomAnchor),
   controls.topAnchor.constraint(equalTo:scroll.contentLayoutGuide.topAnchor),controls.bottomAnchor.constraint(equalTo:scroll.contentLayoutGuide.bottomAnchor,constant:-12),controls.leadingAnchor.constraint(equalTo:scroll.contentLayoutGuide.leadingAnchor,constant:16),controls.trailingAnchor.constraint(equalTo:scroll.contentLayoutGuide.trailingAnchor,constant:-16),controls.widthAnchor.constraint(equalTo:scroll.frameLayoutGuide.widthAnchor,constant:-32)
  ]); changeStep()
 }
 @objc private func changeStep() {
  let descriptions = ["按本人左右选择左耳入口附近实测三角面（蓝色）。轻点增删；多面需相邻且无孔。", "选择右耳入口附近实测三角面（橙色）。不能用未测量补洞或耳后表面代替。", "旋转模型，轻点鼻尖，确认头部前方。", "轻点头顶，确认向上方向。"]
  message.text = descriptions[step.selectedSegmentIndex]+" 双指缩放、拖动旋转。人工确认不代表扫描精度已合格。"
  advanceButton.setTitle(step.selectedSegmentIndex == 3 ? "确认测量位置" : "下一步",for:.normal)
  updateSelection()
 }
 @objc private func selectSurface(_ gesture: UITapGestureRecognizer) {
  // Original mesh only: overlay primitive IDs must not become source-face IDs.
  let hits = sceneView.hitTest(gesture.location(in:sceneView),options:[.rootNode:model,.searchMode:SCNHitTestSearchMode.closest.rawValue,.backFaceCulling:false])
  guard let hit = hits.first(where:{$0.node === model}) else { return }
  do {
   let point = SIMD3<Float>(hit.localCoordinates.x,hit.localCoordinates.y,hit.localCoordinates.z)
   let landmark = try surface.landmark(face:hit.faceIndex,point:point), face = UInt32(landmark.face)
   switch step.selectedSegmentIndex {
   case 0: if left.contains(face) { left.remove(face) } else if left.count < 1024 && !right.contains(face) { left.insert(face) }
   case 1: if right.contains(face) { right.remove(face) } else if right.count < 1024 && !left.contains(face) { right.insert(face) }
   case 2: nose = landmark
   default: crown = landmark
   }
   updateSelection()
  } catch { message.text = error.localizedDescription }
 }
 private func updateSelection() {
  selection.childNodes.forEach { $0.removeFromParentNode() }
  for (faces,color) in [(left,UIColor.systemBlue),(right,UIColor.systemOrange)] where !faces.isEmpty {
   let ids = faces.sorted().flatMap { face in Array(surface.triangles[Int(face)*3..<Int(face)*3+3]) }
   let element = SCNGeometryElement(data:ids.withUnsafeBufferPointer { Data(buffer:$0) },primitiveType:.triangles,primitiveCount:ids.count/3,bytesPerIndex:4)
   let geometry = SCNGeometry(sources:model.geometry!.sources,elements:[element]), material = SCNMaterial()
   material.diffuse.contents = color; material.lightingModel = .constant; material.isDoubleSided = true; material.writesToDepthBuffer = false
   geometry.materials = [material]; let node = SCNNode(geometry:geometry); node.renderingOrder = 10; selection.addChildNode(node)
  }
  for (landmark,color) in [(nose,UIColor.systemGreen),(crown,UIColor.systemPurple)] {
   guard let landmark else { continue }; let geometry = SCNSphere(radius:0.0015); geometry.firstMaterial?.diffuse.contents = color
   let node = SCNNode(geometry:geometry); node.simdPosition = landmark.point; selection.addChildNode(node)
  }
  advanceButton.isEnabled = [!left.isEmpty,!right.isEmpty,nose != nil,crown != nil][step.selectedSegmentIndex]
  sceneView.setNeedsDisplay()
 }
 @objc private func clearStep() {
  switch step.selectedSegmentIndex { case 0:left.removeAll(); case 1:right.removeAll(); case 2:nose = nil; default:crown = nil }; changeStep()
 }
 @objc private func advance() {
  if step.selectedSegmentIndex < 3 { step.selectedSegmentIndex += 1; changeStep(); return }
  guard let nose, let crown else { return }
  do { finish(.success(try surface.confirm(leftFaces:Array(left),rightFaces:Array(right),nose:nose,crown:crown))) }
  catch { message.text = error.localizedDescription }
 }
 @objc private func cancelReview() { cancel() }
 func cancel() { finish(.failure(PersonalHrtfReviewGeometry.Failure(message:"已取消耳入口确认"))) }
 private func finish(_ result: Result<PersonalHrtfReviewGeometry.Confirmation,Error>) {
  guard !ended else { return }; ended = true
  let callback = complete; complete = nil; sceneView.isPlaying = false
  if presentingViewController != nil { dismiss(animated:true) { callback?(result) } }
  else { callback?(result) }
 }
}
