import ExpoModulesCore
import UIKit

#if compiler(>=6.2)
@available(iOS 26.0, *)
@MainActor
private final class RightScrollEdgeConnection: NSObject {
 private weak var owner: UIView?
 private weak var scrollView: UIScrollView?
 private let interaction = UIScrollEdgeElementContainerInteraction()
 private var savedStyle: UIScrollEdgeEffect.Style?
 private var savedHidden = false

 init(owner: UIView) {
  self.owner = owner
  super.init()
  interaction.edge = .right
 }

 func connect(to target: UIScrollView) {
  guard let owner else { return }
  if scrollView !== target {
   detach()
   scrollView = target
   savedStyle = target.rightEdgeEffect.style
   savedHidden = target.rightEdgeEffect.isHidden
  }
  if interaction.view !== owner { owner.addInteraction(interaction) }
  interaction.scrollView = target
  target.rightEdgeEffect.style = .hard
  target.rightEdgeEffect.isHidden = false
 }

 func detach() {
  interaction.scrollView = nil
  if let target = scrollView {
   if let savedStyle { target.rightEdgeEffect.style = savedStyle }
   target.rightEdgeEffect.isHidden = savedHidden
  }
  if let owner, interaction.view === owner { owner.removeInteraction(interaction) }
  scrollView = nil
  savedStyle = nil
 }
}

#endif

final class SdaRightScrollEdgeView: ExpoView {
 var scrollViewTag = 0 {
  didSet {
   guard oldValue != scrollViewTag else { return }
   setNeedsLayout()
   connectIfMounted()
   // A tag may arrive in the same mount transaction as the target ScrollView.
   DispatchQueue.main.async { [weak self] in self?.connectIfMounted() }
  }
 }
 private var connection: NSObject?

 required init(appContext: AppContext? = nil) {
  super.init(appContext: appContext)
  backgroundColor = .clear
  clipsToBounds = false
 }

 override func didMoveToWindow() {
  super.didMoveToWindow()
  if window == nil {
   #if compiler(>=6.2)
   if #available(iOS 26.0, *) { (connection as? RightScrollEdgeConnection)?.detach() }
   #endif
  } else { connectIfMounted() }
 }

 override func layoutSubviews() {
  super.layoutSubviews()
  connectIfMounted()
 }

 override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
  let hit = super.hitTest(point, with: event)
  return hit === self ? nil : hit
 }

 private func findScrollView(_ view: UIView, depth: Int = 0) -> UIScrollView? {
  if let scroll = view as? UIScrollView { return scroll }
  guard depth < 8 else { return nil }
  for child in view.subviews {
   if let scroll = findScrollView(child, depth: depth + 1) { return scroll }
  }
  return nil
 }

 private func connectIfMounted() {
  #if compiler(>=6.2)
  guard #available(iOS 26.0, *) else { return }
  guard window != nil, scrollViewTag > 0,
   let target = appContext?.findView(withTag: scrollViewTag, ofType: UIView.self),
   let scroll = findScrollView(target), scroll.window === window else {
   (connection as? RightScrollEdgeConnection)?.detach()
   return
  }
  let link: RightScrollEdgeConnection
  if let existing = connection as? RightScrollEdgeConnection { link = existing }
  else { link = RightScrollEdgeConnection(owner: self); connection = link }
  link.connect(to: scroll)
  #endif
 }
}

public final class SdaScrollEdgeModule: Module {
 public func definition() -> ModuleDefinition {
  Name("SdaScrollEdge")
  Function("isSupported") { () -> Bool in
   #if compiler(>=6.2)
   if #available(iOS 26.0, *) { return true }
   #endif
   return false
  }
  View(SdaRightScrollEdgeView.self) {
   Prop("scrollViewTag") { (view: SdaRightScrollEdgeView, tag: Int) in view.scrollViewTag = tag }
  }
 }
}
