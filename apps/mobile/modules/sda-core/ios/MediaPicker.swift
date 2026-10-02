import ExpoModulesCore
import UIKit
import UniformTypeIdentifiers

/// Keeps the provider URL, not the imported cache URL, as the next picker location.
/// directoryURL is a navigation hint; it does not grant access to the parent folder.
final class MediaPicker: NSObject, UIDocumentPickerDelegate, UIAdaptivePresentationControllerDelegate {
 private var pending: Promise?
 private let prefs = UserDefaults.standard
 private let directoryKey = "sda.mediaPicker.directory"

 func present(from presenter: UIViewController?, promise: Promise) {
  guard pending == nil else { promise.reject("ERR_PICKER_BUSY", "文件选择器已打开"); return }
  guard let presenter, presenter.view.window != nil, presenter.presentedViewController == nil else {
   promise.reject("ERR_PICKER_PRESENT", "当前无法打开文件选择器"); return
  }
  let picker = UIDocumentPickerViewController(forOpeningContentTypes:[.item], asCopy:false)
  picker.allowsMultipleSelection = true
  picker.delegate = self
  picker.modalPresentationStyle = .pageSheet
  picker.presentationController?.delegate = self
  if let saved = prefs.string(forKey:directoryKey), let directory = URL(string:saved), directory.isFileURL {
   picker.directoryURL = directory
  }
  pending = promise
  presenter.present(picker, animated:true)
 }

 func presentationControllerDidDismiss(_ presentationController: UIPresentationController) {
  resolveCancellation()
 }

 func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
  resolveCancellation()
 }

 private func resolveCancellation() {
  let promise = pending; pending = nil
  promise?.resolve(["canceled":true,"assets":[]] as [String:Any])
 }

 func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
  guard let promise = pending else { return }
  pending = nil
  if let first = urls.first {
   prefs.set(first.deletingLastPathComponent().absoluteString, forKey:directoryKey)
  }
  // File-provider downloads/copies may be slow. Never block UIKit's main thread.
  DispatchQueue.global(qos:.userInitiated).async {
   var imported: [URL] = []
   do {
    var assets: [[String:Any]] = []
    for source in urls {
     let scoped = source.startAccessingSecurityScopedResource()
     defer { if scoped { source.stopAccessingSecurityScopedResource() } }
     let folder = FileManager.default.urls(for:.cachesDirectory,in:.userDomainMask)[0]
      .appendingPathComponent("SdaImports",isDirectory:true)
      .appendingPathComponent(UUID().uuidString,isDirectory:true)
     try FileManager.default.createDirectory(at:folder,withIntermediateDirectories:true)
     imported.append(folder)
     let target = folder.appendingPathComponent(source.lastPathComponent)
     var coordinationError: NSError?
     var copyError: Error?
     NSFileCoordinator().coordinate(readingItemAt:source,options:[],error:&coordinationError) { accessible in
      do { try FileManager.default.copyItem(at:accessible,to:target) } catch { copyError = error }
     }
     if let error = coordinationError { throw error }
     if let error = copyError { throw error }
     assets.append(["uri":target.absoluteString,"name":source.lastPathComponent])
    }
    promise.resolve(["canceled":false,"assets":assets] as [String:Any])
   } catch {
    for folder in imported { try? FileManager.default.removeItem(at:folder) }
    promise.reject("ERR_PICKER_IMPORT", "导入媒体失败: \(error.localizedDescription)")
   }
  }
 }
}
