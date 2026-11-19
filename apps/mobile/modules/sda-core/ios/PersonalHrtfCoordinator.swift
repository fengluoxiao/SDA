import UIKit
import UniformTypeIdentifiers

// This coordinator never stops playback, changes the queue or seeks a track.
// Selection is used when the next native engine is created.
final class PersonalHrtfCoordinator: NSObject, UIDocumentPickerDelegate {
 private var completion: ((Result<[String:Any],Error>) -> Void)?
 private weak var presenter: UIViewController?
 private var scanning: PersonalHeadScanController?
 static func topController() -> UIViewController? {
  let windows = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.filter { $0.activationState == .foregroundActive }.flatMap { $0.windows }
  var root = windows.first(where:{$0.isKeyWindow})?.rootViewController
  while let next = root?.presentedViewController { root = next }
  return root
 }
 override init() {
  super.init(); PersonalHrtfStore.prepareForLaunch()
 }
 func cancel() {
  if let scanning { scanning.cancel() }
  else { presenter?.presentedViewController?.dismiss(animated:false); finish(.failure(PersonalHrtfError.message("操作已取消"))) }
 }
 func run(_ action: String, completion: @escaping (Result<[String:Any],Error>) -> Void) {
  guard self.completion == nil, let presenter = Self.topController() else { completion(.failure(PersonalHrtfError.message("已有操作正在进行，请先返回"))); return }
  self.completion = completion; self.presenter = presenter
  switch action {
  case "scan":
   guard PersonalHeadScanController.supported() else { finish(.failure(PersonalHrtfError.message("此设备需要前置 TrueDepth 相机；Expo Go 不包含采集模块"))); return }
   let scanner = PersonalHeadScanController(); scanner.modalPresentationStyle = .fullScreen; scanning = scanner
   scanner.complete = { [weak self] result in
    guard let self else { return }; self.scanning = nil
    switch result {
    case .failure(let error): self.finish(.failure(error))
    case .success(let url):
     let share = UIActivityViewController(activityItems:[url],applicationActivities:nil)
     share.popoverPresentationController?.sourceView = presenter.view
     share.popoverPresentationController?.sourceRect = CGRect(x:presenter.view.bounds.midX,y:presenter.view.bounds.midY,width:1,height:1)
     share.completionWithItemsHandler = { [weak self] _, saved, _, error in
      try? FileManager.default.removeItem(at:url.deletingLastPathComponent())
      if let error { self?.finish(.failure(error)) }
      else { self?.finish(.success(["exported":saved,"scanValidated":false,"message":saved ? "已导出原始扫描；需在电脑重建、检查质量和求解" : "未导出，临时扫描已删除"])) }
     }
     presenter.present(share,animated:true)
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
 func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) { finish(.success(["cancelled":true])) }
 func documentPicker(_ controller: UIDocumentPickerViewController,didPickDocumentsAt urls: [URL]) {
  guard let url = urls.first, url.pathExtension.lowercased() == "phrtf" else { finish(.failure(PersonalHrtfError.message("请选择电脑求解后导出的 .phrtf 档案"))); return }
  DispatchQueue.global(qos:.userInitiated).async { [weak self] in
   let access = url.startAccessingSecurityScopedResource(); defer { if access { url.stopAccessingSecurityScopedResource() } }
   let result = Result { try PersonalHrtfStore.install(url) }
   // UIDocumentPicker asCopy may stage the source in Inbox. Retain only the
   // checked, bounded response bank, never a second archive copy.
   if url.path.contains("/Inbox/") { try? FileManager.default.removeItem(at:url) }
   DispatchQueue.main.async { self?.finish(result) }
  }
 }
 private func finish(_ result: Result<[String:Any],Error>) { let done = completion; completion = nil; done?(result) }
}
