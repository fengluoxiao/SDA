import UIKit

final class PersonalHeadExportPresenter {
 private var busy = false
 func share(_ item:PersonalHeadExport,from host:UIViewController,anchor:UIView?) {
  guard !busy, host.presentedViewController == nil else { return }; busy = true
  DispatchQueue.global(qos:.userInitiated).async { [weak host,weak anchor] in
   let result = Result { try item.makeFile() }
   DispatchQueue.main.async { [weak host,weak anchor] in
    guard let host, host.viewIfLoaded?.window != nil, !host.isBeingDismissed, host.navigationController?.isBeingDismissed != true, host.presentedViewController == nil else { self.busy = false; return }
    switch result {
    case .failure: self.busy = false; Self.showError(host)
    case .success(let file):
     let sheet = UIActivityViewController(activityItems:[file.url],applicationActivities:nil)
     let view = anchor.flatMap { $0.window != nil ? $0 : nil } ?? host.view!
     sheet.popoverPresentationController?.sourceView = view
     sheet.popoverPresentationController?.sourceRect = CGRect(x:view.bounds.midX,y:view.bounds.midY,width:1,height:1)
     sheet.completionWithItemsHandler = { [weak host,file] _,_,_,error in
      // Retain the file until the activity (including Save to Files) has consumed it.
      file.discard(); self.busy = false
      if error != nil, let host { DispatchQueue.main.async { Self.showError(host) } }
     }
     host.present(sheet,animated:true)
    }
   }
  }
 }
 private static func showError(_ host:UIViewController) {
  guard host.viewIfLoaded?.window != nil, host.presentedViewController == nil else { return }
  let alert = UIAlertController(title:"导出失败",message:"无法完成文件导出，可以重试。",preferredStyle:.alert)
  alert.addAction(UIAlertAction(title:"好",style:.default)); host.present(alert,animated:true)
 }
}

// Stay on this sheet when an activity finishes so both files can be exported
// independently, without keeping a scan after the user closes the sheet.
final class PersonalHeadScanFailureController: UITableViewController {
 private let report:PersonalHeadReconstructionFailure, exporter = PersonalHeadExportPresenter()
 var closed:(()->Void)?
 init(_ report:PersonalHeadReconstructionFailure) { self.report = report; super.init(style:.insetGrouped) }
 required init?(coder:NSCoder) { fatalError() }
 override func viewDidLoad() {
  super.viewDidLoad(); title = "个人 HRTF 未完成"
  navigationItem.rightBarButtonItem = UIBarButtonItem(barButtonSystemItem:.done,target:self,action:#selector(close))
 }
 @objc func close() {
  guard let callback = closed else { return }; closed = nil
  if let host = navigationController?.presentingViewController { host.dismiss(animated:true,completion:callback) }
  else { callback() }
 }
 override func tableView(_ tableView:UITableView,numberOfRowsInSection section:Int)->Int { report.geometry == nil ? 1 : 2 }
 override func tableView(_ tableView:UITableView,titleForHeaderInSection section:Int)->String? { report.message }
 override func tableView(_ tableView:UITableView,titleForFooterInSection section:Int)->String? { "日志不含照片或三维坐标。模型或点云包含个人头耳形状，仅在你选择导出时分享。关闭后会删除本次临时数据。" }
 private func item(at path:IndexPath)->PersonalHeadExport { path.row == 0 ? .log(report.log) : report.geometry! }
 override func tableView(_ tableView:UITableView,cellForRowAt path:IndexPath)->UITableViewCell {
  let cell = UITableViewCell(style:.default,reuseIdentifier:nil); var content = cell.defaultContentConfiguration()
  content.text = item(at:path).title; content.image = UIImage(systemName:path.row == 0 ? "doc.text" : "cube"); cell.contentConfiguration = content
  cell.accessibilityTraits.insert(.button); return cell
 }
 override func tableView(_ tableView:UITableView,didSelectRowAt path:IndexPath) {
  tableView.deselectRow(at:path,animated:true); exporter.share(item(at:path),from:self,anchor:tableView.cellForRow(at:path))
 }
}
