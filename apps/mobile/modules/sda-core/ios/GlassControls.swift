import UIKit

private let sdaAccent = UIColor { traits in
 traits.userInterfaceStyle == .dark
  ? UIColor(red: 159/255, green: 220/255, blue: 185/255, alpha: 1)
  : UIColor(red: 40/255, green: 117/255, blue: 74/255, alpha: 1)
}

// UIKit owns the material, SF Symbols, highlighting and accessibility. Keep
// these controls independent of the decoder and of the React render cadence.
final class GlassButtonControl: UIView {
 let button = UIButton(type: .system)
 var action: (() -> Void)?
 var symbol = "plus" { didSet { if symbol != oldValue { update() } } }
 var title = "" { didSet { if title != oldValue { update() } } }
 var subtitle = "" { didSet { if subtitle != oldValue { update() } } }
 var row = false { didSet { if row != oldValue { update() } } }
 var selected = false { didSet { if selected != oldValue { update() } } }
 var choices: [String] = [] { didSet { if choices != oldValue { updateMenu() } } }
 var choiceIndex = -1 { didSet { if choiceIndex != oldValue { updateMenu() } } }
 var choose: ((Int) -> Void)?
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
  materialKind = "legacyButton"
  var config = prominent ? UIButton.Configuration.filled() : UIButton.Configuration.gray()
  #if compiler(>=6.2)
  if #available(iOS 26.0, *), NSClassFromString("UIGlassEffect") != nil {
   config = prominent ? .prominentGlass() : .glass()
   materialKind = "liquidGlass"
  }
  #endif
  if row {
   config = .plain()
   config.background.backgroundColor = selected ? sdaAccent.withAlphaComponent(0.12) : .clear
   config.background.cornerRadius = 14
   materialKind = "nativeRow"
  }
  config.title = title.isEmpty ? nil : title
  config.subtitle = subtitle.isEmpty ? nil : subtitle
  config.titleLineBreakMode = .byTruncatingTail
  config.subtitleLineBreakMode = .byTruncatingTail
  config.titleAlignment = .leading
  config.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { attributes in
   var result = attributes; result.font = UIFont.preferredFont(forTextStyle: .body); return result
  }
  config.subtitleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { attributes in
   var result = attributes; result.font = UIFont.preferredFont(forTextStyle: .caption1); result.foregroundColor = UIColor.secondaryLabel; return result
  }
  config.imagePadding = 10
  config.titlePadding = 4
  config.cornerStyle = .capsule
  config.contentInsets = title.isEmpty ? .zero : NSDirectionalEdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14)
  config.image = UIImage(systemName: symbol, withConfiguration: UIImage.SymbolConfiguration(pointSize: symbolSize, weight: .semibold))
  config.baseForegroundColor = prominent ? .white : .label
  if prominent { config.baseBackgroundColor = sdaAccent }
  button.configuration = config
  button.contentHorizontalAlignment = row ? .leading : .center
  button.accessibilityTraits = selected ? [.button, .selected] : [.button]
  button.isEnabled = enabled
  button.accessibilityLabel = label
  button.accessibilityIdentifier = "sda.chrome." + label
 }
 private func updateMenu() {
  button.showsMenuAsPrimaryAction = !choices.isEmpty
  button.menu = choices.isEmpty ? nil : UIMenu(children: choices.enumerated().map { index, name in
   UIAction(title: name, state: index == choiceIndex ? .on : .off) { [weak self] _ in self?.choose?(index) }
  })
 }
 @objc private func pressed() { if choices.isEmpty { action?() } }
}

final class GlassTabsControl: UIView {
 let effectView = UIVisualEffectView()
 let stack = UIStackView()
 private let titles = ["播放", "资料库", "空间"]
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
   config.baseForegroundColor = index == selected ? sdaAccent : .secondaryLabel
   config.background.backgroundColor = index == selected ? sdaAccent.withAlphaComponent(0.09) : .clear
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

// Keep the native thumb under the finger despite frequent React status updates.
final class NativeSliderControl: UIView {
 let slider = UISlider()
 var changed: ((Double) -> Void)?
 var tracking: ((Bool) -> Void)?
 var value: Double = 1 { didSet { if !slider.isTracking { slider.value = Float(value) } } }
 override init(frame: CGRect) {
  super.init(frame: frame); addSubview(slider)
  slider.minimumValue = 0; slider.maximumValue = 1
  slider.minimumValueImage = UIImage(systemName: "speaker.fill")
  slider.maximumValueImage = UIImage(systemName: "speaker.wave.3.fill")
  slider.tintColor = sdaAccent; slider.accessibilityLabel = "音量"
  slider.addTarget(self, action: #selector(started), for: .touchDown)
  slider.addTarget(self, action: #selector(moved), for: .valueChanged)
  slider.addTarget(self, action: #selector(ended), for: [.touchUpInside, .touchUpOutside, .touchCancel])
 }
 required init?(coder: NSCoder) { fatalError("init(coder:) is unsupported") }
 override func layoutSubviews() { super.layoutSubviews(); slider.frame = bounds }
 override func didMoveToWindow() { super.didMoveToWindow(); if window == nil { tracking?(false) } }
 @objc private func started() { tracking?(true) }
 @objc private func moved() { changed?(Double(slider.value)) }
 @objc private func ended() { changed?(Double(slider.value)); tracking?(false) }
}
final class NativeStepperControl: UIView {
 let stepper = UIStepper()
 var changed: ((Double) -> Void)?
 override init(frame: CGRect) {
  super.init(frame: frame); addSubview(stepper)
  stepper.minimumValue = 0.25; stepper.maximumValue = 4; stepper.stepValue = 0.05
  stepper.tintColor = sdaAccent; stepper.accessibilityLabel = "距离映射，单位米"
  stepper.addTarget(self, action: #selector(moved), for: .valueChanged)
 }
 required init?(coder: NSCoder) { fatalError("init(coder:) is unsupported") }
 override func layoutSubviews() { super.layoutSubviews(); stepper.sizeToFit(); stepper.center = CGPoint(x: bounds.midX, y: bounds.midY) }
 @objc private func moved() { changed?((stepper.value * 100).rounded() / 100) }
}
