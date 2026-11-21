import UIKit
import UniformTypeIdentifiers

// All processing implementations execute inside the iOS application. A mesh
// candidate is never accepted directly as a personal HRTF. Implementations must
// call completion only after all native writes/threads have stopped.
protocol PersonalHrtfLocalPipeline: AnyObject {
 func makeReconstruction() throws -> PersonalHeadReconstructor
 func process(_ candidate: PersonalHeadMeshCandidate, landmarks: PersonalHrtfReviewGeometry.Confirmation, completion: @escaping (Result<[String:Any],Error>) -> Void)
 func generate(_ candidate:PersonalHeadMeshCandidate, landmarks:PersonalHrtfReviewGeometry.Confirmation, root:URL,
               progress:@escaping(String)->Void,completion:@escaping(Result<[String:Any],Error>)->Void)
 func cancel()
}

// This coordinator never stops playback, changes the queue or seeks a track.
// Selection is used when the next native engine is created.
final class PersonalHrtfCoordinator: NSObject, UIDocumentPickerDelegate {
 private var completion: ((Result<[String:Any],Error>) -> Void)?
 private weak var presenter: UIViewController?
 private var scanning: PersonalHeadScanController?
 private let localPipeline: PersonalHrtfLocalPipeline?
 private var processing = false
 private var jobRoot:URL?, jobAlert:UIAlertController?, installing=false
 private var jobObservers:[NSObjectProtocol]=[]
 private var review: PersonalHrtfReviewController?
 private weak var failureSheet:PersonalHeadScanFailureController?
 static func topController() -> UIViewController? {
  let windows = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.filter { $0.activationState == .foregroundActive }.flatMap { $0.windows }
  var root = windows.first(where:{$0.isKeyWindow})?.rootViewController
  while let next = root?.presentedViewController { root = next }
  return root
 }
 init(localPipeline: PersonalHrtfLocalPipeline? = nil) {
  self.localPipeline = localPipeline; super.init(); PersonalHrtfStore.prepareForLaunch()
 }
 func cancel() {
  if installing { return }
  if let failureSheet { failureSheet.close() }
  else if let review { review.cancel() }
  else if let scanning { scanning.cancel() }
  else if processing { localPipeline?.cancel() }
  else if let jobAlert {jobAlert.dismiss(animated:false);finish(.success(["cancelled":true]))}
  else { presenter?.presentedViewController?.dismiss(animated:false); finish(.failure(PersonalHrtfError.message("操作已取消"))) }
 }
 func run(_ action: String, completion: @escaping (Result<[String:Any],Error>) -> Void) {
  guard self.completion == nil, let presenter = Self.topController() else { completion(.failure(PersonalHrtfError.message("已有操作正在进行，请先返回"))); return }
  self.completion = completion; self.presenter = presenter
  switch action {
  case "scan", "scanExperimental":
   guard PersonalHeadScanController.supported() else { finish(.failure(PersonalHrtfError.message("此设备需要前置 TrueDepth 相机；Expo Go 不包含采集模块"))); return }
   guard let localPipeline else { finish(.failure(PersonalHrtfError.message("此构建尚未包含完整的本机重建与求解流程，个人采集暂不可用"))); return }
   let reconstruction: PersonalHeadReconstructor
   do { reconstruction = try localPipeline.makeReconstruction() }
   catch { finish(.failure(error)); return }
   let scanner = PersonalHeadScanController(reconstruction:reconstruction); scanner.modalPresentationStyle = .fullScreen; scanning = scanner
   scanner.complete = { [weak self] result in
    guard let self else { return }; self.scanning = nil
    switch result {
    case .failure(let error):
     self.presentFailure(error)
    case .success(let candidate):
     let review = PersonalHrtfReviewController(surface:candidate.surface)
     let exporter = PersonalHeadExportPresenter()
     review.exportSurface = { host,button in exporter.share(.mesh(candidate.surface,verified:true),from:host,anchor:button) }
     review.modalPresentationStyle = .fullScreen; self.review = review
     review.complete = { [self] result in
      self.review = nil
      switch result {
      case .failure(let error):
       self.finish(.failure(error))
      case .success(let landmarks):
       if action == "scanExperimental" { self.generate(candidate,landmarks,localPipeline);return }
       self.processing = true
       localPipeline.process(candidate,landmarks:landmarks) { [self] result in
        DispatchQueue.main.async {
         // The pipeline has finished every writer before releasing its input.
         self.processing = false; self.finish(result)
        }
       }
      }
     }
     guard let presenter = Self.topController(), presenter.viewIfLoaded?.window != nil else {
      self.review = nil; review.complete = nil
      self.finish(.failure(PersonalHrtfError.message("采集已结束，请回到应用重新确认测量位置"))); return
     }
     presenter.present(review,animated:true)
    }
   }
   presenter.present(scanner,animated:true)
  case "import":
   let picker = UIDocumentPickerViewController(forOpeningContentTypes:[.data],asCopy:true)
   picker.allowsMultipleSelection = false; picker.delegate = self; presenter.present(picker,animated:true)
  case "default": PersonalHrtfStore.useDefault(); finish(.success(PersonalHrtfStore.status()))
  case "delete": do { try PersonalHrtfStore.remove(); finish(.success(PersonalHrtfStore.status())) } catch { finish(.failure(error)) }
  default: finish(.failure(PersonalHrtfError.message("未知个人 HRTF 操作")))
  }
 }
 private func presentFailure(_ error:Error) {
  if error is CancellationError {finish(.success(["cancelled":true]));return}
  guard let report=error as? PersonalHeadReconstructionFailure else {finish(.failure(error));return}
  guard UIApplication.shared.applicationState == .active, let host=Self.topController(),host.viewIfLoaded?.window != nil else {finish(.failure(PersonalHrtfError.message(report.message)));return}
  let sheet=PersonalHeadScanFailureController(report), nav=UINavigationController(rootViewController:sheet)
  nav.isModalInPresentation=true;failureSheet=sheet
  sheet.closed={ [weak self] in self?.failureSheet=nil;self?.finish(.success(["failurePresented":true])) }
  host.present(nav,animated:true)
 }
 private func generate(_ candidate:PersonalHeadMeshCandidate,_ landmarks:PersonalHrtfReviewGeometry.Confirmation,_ pipeline:PersonalHrtfLocalPipeline){
  guard let host=Self.topController(),host.viewIfLoaded?.window != nil else { finish(.failure(PersonalHrtfError.message("请在前台开始计算")));return }
  let root=FileManager.default.temporaryDirectory.appendingPathComponent("SDAHeadScan",isDirectory:true).appendingPathComponent(UUID().uuidString,isDirectory:true)
  do { try FileManager.default.createDirectory(at:root,withIntermediateDirectories:true) } catch { finish(.failure(error));return }
  jobRoot=root;processing=true
  let alert=UIAlertController(title:"本机计算实验档案",message:"正在检查实测头耳表面",preferredStyle:.alert)
  jobAlert=alert;alert.addAction(UIAlertAction(title:"取消计算",style:.cancel){ _ in pipeline.cancel() })
  let center=NotificationCenter.default
  for name in [UIApplication.didEnterBackgroundNotification,UIApplication.didReceiveMemoryWarningNotification,ProcessInfo.thermalStateDidChangeNotification] {
   jobObservers.append(center.addObserver(forName:name,object:nil,queue:.main){ [weak self] _ in
    if name != ProcessInfo.thermalStateDidChangeNotification || ProcessInfo.processInfo.thermalState.rawValue >= ProcessInfo.ThermalState.serious.rawValue {self?.cancel()}
   })
  }
  host.present(alert,animated:false)
  pipeline.generate(candidate,landmarks:landmarks,root:root,progress:{ [weak alert] text in alert?.message=text }) { [self] outcome in
   DispatchQueue.main.async {
    self.processing=false;self.jobAlert=nil
    let done={ [self] in
     switch outcome {
     case .failure(let error):self.clearJob();self.presentFailure(error)
     case .success(let details):
      guard self.jobRoot==root, UIApplication.shared.applicationState == .active,let host=Self.topController(),host.viewIfLoaded?.window != nil else { self.finish(.success(["cancelled":true]));return }
      let boundary=(details["boundaryFaces"] as? Int ?? 0)>0 ? "后方使用计算边界，并非实测后脑。" : "使用闭合实测表面。"
      let sheet=UIAlertController(title:"实验档案已生成",message:"\(details["directions"] as? Int ?? 0) 个方向。\(boundary)真人声学精度未经验证。",preferredStyle:.alert)
      self.jobAlert=sheet
      sheet.addAction(UIAlertAction(title:"保留当前档案",style:.cancel){ _ in self.finish(.success(["cancelled":true])) })
      sheet.addAction(UIAlertAction(title:"启用实验档案",style:.default){ _ in
       self.installing=true
       DispatchQueue.global(qos:.utility).async {
        let result=Result { try PersonalHrtfStore.install(root.appendingPathComponent("response.phrtf")) }
        DispatchQueue.main.async { self.installing=false;self.finish(result) }
       }
      });host.present(sheet,animated:true)
     }
    }
    if alert.presentingViewController != nil { alert.dismiss(animated:false,completion:done) } else { done() }
   }
  }
 }
 func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) { finish(.success(["cancelled":true])) }
 func documentPicker(_ controller: UIDocumentPickerViewController,didPickDocumentsAt urls: [URL]) {
  guard let url = urls.first, url.pathExtension.lowercased() == "phrtf" else { finish(.failure(PersonalHrtfError.message("请选择完整且已校验的 .phrtf 个人档案"))); return }
  DispatchQueue.global(qos:.userInitiated).async { [weak self] in
   let access = url.startAccessingSecurityScopedResource(); defer { if access { url.stopAccessingSecurityScopedResource() } }
   let result = Result { try PersonalHrtfStore.install(url) }
   // UIDocumentPicker asCopy may stage the source in Inbox. Retain only the
   // checked, bounded response bank, never a second archive copy.
   if url.path.contains("/Inbox/") { try? FileManager.default.removeItem(at:url) }
   DispatchQueue.main.async { self?.finish(result) }
  }
 }
 private func clearJob() {
  for observer in jobObservers { NotificationCenter.default.removeObserver(observer) };jobObservers.removeAll();jobAlert=nil
  if let root=jobRoot {try? FileManager.default.removeItem(at:root);jobRoot=nil}
 }
 private func finish(_ result: Result<[String:Any],Error>) {
  clearJob()
  let done=completion;completion=nil;done?(result)
 }
}
