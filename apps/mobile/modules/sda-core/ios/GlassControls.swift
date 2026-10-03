import UIKit

// UIKit owns the material, SF Symbols, highlighting and accessibility. Keep
// these controls independent of the decoder and of the React render cadence.
final class GlassButtonControl: UIView {
 let button = UIButton(type: .system)
 var action: (() -> Void)?
 var symbol = "plus" { didSet { if symbol != oldValue { update() } } }
 var label = "" { didSet { if label != oldValue { update() } } }
 var enabled = true { didSet { button.isEnabled = enabled } }
 var prominent = false { didSet { if prominent != oldValue { update() } } }
 var symbolSize: CGFloat = 20 { didSet { if symbolSize != oldValue { update() } } }
 private(set) var materialKind = "legacyButton"
 override init(frame: CGRect) {
  super.init(frame: frame)
  addSubview(button)
  button.addTarget(self, action: #selector(pressed), for: .touchUpInside)
  update()
 }
 required init?(coder: NSCoder) { fatalError("init(coder:) is unsupported") }
 override func layoutSubviews() { super.layoutSubviews(); button.frame = bounds }
 private func update() {
  var config = prominent ? UIButton.Configuration.filled() : UIButton.Configuration.gray()
  #if compiler(>=6.2)
  if #available(iOS 26.0, *), NSClassFromString("UIGlassEffect") != nil {
   config = prominent ? .prominentGlass() : .glass()
   materialKind = "liquidGlass"
  }
  #endif
  config.cornerStyle = .capsule
  config.contentInsets = .zero
  config.image = UIImage(systemName: symbol, withConfiguration: UIImage.SymbolConfiguration(pointSize: symbolSize, weight: .semibold))
  config.baseForegroundColor = prominent ? .white : .label
  if prominent { config.baseBackgroundColor = .systemTeal }
  button.configuration = config
  button.isEnabled = enabled
  button.accessibilityLabel = label
  button.accessibilityIdentifier = "sda.chrome." + label
 }
 @objc private func pressed() { action?() }
}

final class GlassTabsControl: UIView {
 let effectView = UIVisualEffectView()
 let stack = UIStackView()
 private let titles = ["播放", "列表", "空间"]
 private let symbols = ["play.circle.fill", "music.note.list", "cube.transparent"]
 private var buttons: [UIButton] = []
 var action: ((Int) -> Void)?
 var selected = 0 { didSet { if selected != oldValue { updateButtons() } } }
 private(set) var materialKind = "legacyMaterial"
 private var installed = false
 private var observer: NSObjectProtocol?
 override init(frame: CGRect) {
  super.init(frame: frame)
  effectView.clipsToBounds = true
  addSubview(effectView)
  stack.axis = .horizontal; stack.distribution = .fillEqually; stack.spacing = 4
  effectView.contentView.addSubview(stack)
  for index in titles.indices {
   let button = UIButton(type: .system)
   button.tag = index
   button.addTarget(self, action: #selector(pressed(_:)), for: .touchUpInside)
   buttons.append(button); stack.addArrangedSubview(button)
  }
  observer = NotificationCenter.default.addObserver(forName: UIAccessibility.reduceTransparencyStatusDidChangeNotification, object: nil, queue: .main) { [weak self] _ in
   self?.installed = false; self?.setNeedsLayout()
  }
  updateButtons()
 }
 required init?(coder: NSCoder) { fatalError("init(coder:) is unsupported") }
 deinit { if let observer = observer { NotificationCenter.default.removeObserver(observer) } }
 override func didMoveToWindow() { super.didMoveToWindow(); installed = false; setNeedsLayout() }
 override func layoutSubviews() {
  super.layoutSubviews()
  effectView.frame = bounds
  effectView.layer.cornerRadius = bounds.height / 2
  stack.frame = bounds.insetBy(dx: 6, dy: 6)
  // Install only after a mounted, nonzero view, not during Expo construction.
  if !installed, window != nil, bounds.width > 0, bounds.height > 0 {
   installed = true
   effectView.effect = nil
   effectView.backgroundColor = .clear
   if UIAccessibility.isReduceTransparencyEnabled {
    materialKind = "reducedTransparency"; effectView.backgroundColor = .secondarySystemBackground
   } else {
    effectView.effect = UIBlurEffect(style: .systemChromeMaterial)
    materialKind = "legacyMaterial"
    #if compiler(>=6.2)
    if #available(iOS 26.0, *), let type = NSClassFromString("UIGlassEffect") as? NSObject.Type,
       type.responds(to: NSSelectorFromString("effectWithStyle:")) {
     let glass = UIGlassEffect(style: .regular)
     glass.isInteractive = true
     effectView.effect = glass; materialKind = "liquidGlass"
    }
    #endif
   }
  }
 }
 private func updateButtons() {
  for (index, button) in buttons.enumerated() {
   var config = UIButton.Configuration.plain()
   config.title = titles[index]
   config.image = UIImage(systemName: symbols[index], withConfiguration: UIImage.SymbolConfiguration(pointSize: 17, weight: .medium))
   config.imagePlacement = .top; config.imagePadding = 3
   config.contentInsets = .zero
   config.cornerStyle = .capsule
   config.baseForegroundColor = index == selected ? .label : .secondaryLabel
   config.background.backgroundColor = index == selected ? UIColor.label.withAlphaComponent(0.09) : .clear
   config.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { attributes in
    var result = attributes
    result.font = UIFont.preferredFont(forTextStyle: .caption1)
    return result
   }
   button.configuration = config
   button.accessibilityLabel = titles[index]
   button.accessibilityIdentifier = "sda.tab." + String(index)
   button.accessibilityTraits = index == selected ? [.button, .selected] : [.button]
  }
 }
 @objc private func pressed(_ button: UIButton) {
  selected = button.tag
  action?(button.tag)
 }
}

final class MaterialSurfaceControl: UIView {
 let effectView = UIVisualEffectView(effect: UIBlurEffect(style: .systemMaterial))
 private var observer: NSObjectProtocol?
 override init(frame: CGRect) {
  super.init(frame: frame)
  isUserInteractionEnabled = false
  addSubview(effectView)
  observer = NotificationCenter.default.addObserver(forName: UIAccessibility.reduceTransparencyStatusDidChangeNotification, object: nil, queue: .main) { [weak self] _ in self?.update() }
  update()
 }
 required init?(coder: NSCoder) { fatalError("init(coder:) is unsupported") }
 deinit { if let observer = observer { NotificationCenter.default.removeObserver(observer) } }
 override func layoutSubviews() { super.layoutSubviews(); effectView.frame = bounds }
 private func update() {
  effectView.effect = UIAccessibility.isReduceTransparencyEnabled ? nil : UIBlurEffect(style: .systemMaterial)
  effectView.backgroundColor = UIAccessibility.isReduceTransparencyEnabled ? .systemGroupedBackground : .clear
 }
}
