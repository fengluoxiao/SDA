import UIKit
import SceneKit

enum ReviewResult {
 static func write(_ result: [String:Any]) {
  guard let directory = FileManager.default.urls(for:.documentDirectory,in:.userDomainMask).first else { return }
  try? FileManager.default.createDirectory(at:directory,withIntermediateDirectories:true)
  if let data = try? JSONSerialization.data(withJSONObject:result,options:[.sortedKeys]) {
   try? data.write(to:directory.appendingPathComponent("review-result.json"),options:.atomic)
  }
 }
}
@objc(ReviewTestDelegate) final class ReviewTestDelegate: UIResponder, UIApplicationDelegate {
 func application(_ application: UIApplication,didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey:Any]? = nil) -> Bool {
  fputs("Review app launched\n",stderr)
  DispatchQueue.global().asyncAfter(deadline:.now()+90) {
   ReviewResult.write(["passed":false,"error":"review app watchdog expired"]); fputs("Review app watchdog expired\n",stderr); exit(1)
  }
  return true
 }
}
@objc(ReviewSceneDelegate) final class ReviewSceneDelegate: UIResponder, UIWindowSceneDelegate {
 var window: UIWindow?
 private var callbacks = 0
 func scene(_ scene: UIScene,willConnectTo session: UISceneSession,options: UIScene.ConnectionOptions) {
  guard let scene = scene as? UIWindowScene else { fail("missing window scene") }
  fputs("Review window scene connected\n",stderr)
  let window = UIWindow(windowScene:scene), host = UIViewController()
  window.rootViewController = host; self.window = window; window.makeKeyAndVisible()
  DispatchQueue.main.asyncAfter(deadline:.now()+30) { self.fail("review UI timed out") }
  DispatchQueue.main.async {
   do {
    try ScanExportTest.run()
    var earlyCallbacks = 0
    let early = PersonalHrtfReviewController(surface:try ReviewFixture.surface())
    early.complete = { _ in earlyCallbacks += 1 }
    early.cancel(); early.cancel()
    try ReviewFixture.require(earlyCallbacks == 1,"cancel before presentation")
    fputs("Creating measured review controller\n",stderr)
    let controller = PersonalHrtfReviewController(surface:try ReviewFixture.surface())
    var exportTapped = false
    controller.exportSurface = { _,_ in exportTapped = true }
    controller.modalPresentationStyle = .fullScreen
    controller.complete = { [self] result in
     callbacks += 1
     guard case .failure = result, callbacks == 1 else { self.fail("cancel completion") }
     DispatchQueue.main.asyncAfter(deadline:.now()+0.2) {
      guard self.callbacks == 1, host.presentedViewController == nil else { self.fail("cancel dismissal") }
      guard exportTapped else { self.fail("review export action missing") }
      self.checkFailureSheet(host)
     }
    }
    host.present(controller,animated:false) {
     fputs("Review controller presented\n",stderr)
     do { try self.check(controller) } catch { self.fail(error.localizedDescription) }
     self.descendants(controller.view).compactMap { $0 as? UIButton }.first { $0.configuration?.title == "导出实测模型" }?.sendActions(for:.touchUpInside)
     controller.cancel(); controller.cancel()
    }
   } catch { self.fail(error.localizedDescription) }
  }
 }
 private func descendants(_ view: UIView) -> [UIView] { [view]+view.subviews.flatMap(descendants) }
 private func checkFailureSheet(_ host:UIViewController) {
  let log = PersonalHeadScanLog(), index = IndexPath(row:0,section:0)
  guard let surface=try? ReviewFixture.surface() else {fail("missing review fixture")}
  log.stage="prepare-acoustic-input";log.boundaryIssue="insufficient-interior-depth";log.boundaryRegion="left-ear"
  let sheet = PersonalHeadScanFailureController(.init(message:"合成测试",log:log,geometry:.mesh(surface,verified:true),independentPoints:.points([0,0,0],[0,0,1])))
  let nav = UINavigationController(rootViewController:sheet)
  host.present(nav,animated:false) {
   let titles=(0..<sheet.tableView.numberOfRows(inSection:0)).map {row in (sheet.tableView(sheet.tableView,cellForRowAt:IndexPath(row:row,section:0)).contentConfiguration as? UIListContentConfiguration)?.text ?? ""}
   guard titles == ["导出日志","导出实测模型","导出实测点云（无网格）"], sheet.tableView(sheet.tableView,cellForRowAt:index).accessibilityTraits.contains(.button) else { self.fail("missing independent export choices") }
   sheet.tableView(sheet.tableView,didSelectRowAt:index)
   func checkShare(_ attempt:Int) {
    if let share = sheet.presentedViewController as? UIActivityViewController {
     share.dismiss(animated:false) {
      let result:[String:Any] = ["passed":true,"layoutSizesChecked":6,"cancelDismissedOnce":true,"cancelBeforePresentation":true,"failureSharePresented":true,"meshAndCloudRoundTrip":true,"privateLogAndCleanup":true,"physicalScanTested":false,"deviceVisualAcceptance":false]
      ReviewResult.write(result); print(String(decoding:try! JSONSerialization.data(withJSONObject:result),as:UTF8.self)); fflush(stdout); exit(0)
     }
    } else if attempt<50 { DispatchQueue.main.asyncAfter(deadline:.now()+0.1){checkShare(attempt+1)} }
    else { self.fail("Native share sheet was not presented") }
   }
   checkShare(0)
  }
 }
 private func check(_ controller: PersonalHrtfReviewController) throws {
  let original = controller.view.frame
  defer { controller.view.frame = original }
  for size in [CGSize(width:393,height:852),CGSize(width:852,height:393),CGSize(width:320,height:568),CGSize(width:568,height:320),CGSize(width:1024,height:1366),CGSize(width:1366,height:1024)] {
   controller.view.frame = CGRect(origin:.zero,size:size); controller.view.setNeedsLayout(); controller.view.layoutIfNeeded()
   let all = descendants(controller.view), scene = all.compactMap { $0 as? SCNView }.first!
   let segment = all.compactMap { $0 as? UISegmentedControl }.first!
   let scroll = all.compactMap { $0 as? UIScrollView }.first!
   let cancel = all.compactMap { $0 as? UIButton }.first { $0.currentTitle == "取消并删除采集" }!
   try ReviewFixture.require(scene.bounds.width > 0 && scene.bounds.height > 0 && scroll.bounds.height > 0,"viewport collapsed")
   try ReviewFixture.require(!scene.hasAmbiguousLayout && !segment.hasAmbiguousLayout && !scroll.hasAmbiguousLayout,"ambiguous review layout")
   let cancelBounds = cancel.convert(cancel.bounds,to:scroll)
   try ReviewFixture.require(cancelBounds.maxY <= scroll.contentSize.height+1 && cancelBounds.minX >= -1 && cancelBounds.maxX <= scroll.bounds.width+1,"cancel outside scrollable content")
   try ReviewFixture.require(!scene.rendersContinuously && !scene.isPlaying,"unnecessary continuous rendering")
   try ReviewFixture.require(segment.numberOfSegments == 4,"landmark steps missing")
  }
 }
 private func fail(_ message: String) -> Never { ReviewResult.write(["passed":false,"error":message]); fputs("Review UI test: "+message+"\n",stderr); exit(1) }
}
@main struct ReviewUITest {
 static func main() { UIApplicationMain(CommandLine.argc,CommandLine.unsafeArgv,nil,NSStringFromClass(ReviewTestDelegate.self)) }
}
