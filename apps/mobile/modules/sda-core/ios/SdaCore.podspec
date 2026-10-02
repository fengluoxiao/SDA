Pod::Spec.new do |s|
 s.name = 'SdaCore'
 s.version = '0.1.0'
 s.summary = 'Shared SDA Rust decoder and KU100 renderer for iOS'
 s.description = s.summary
 s.license = { :type => 'GPL-3.0-or-later' }
 s.author = 'SDA contributors'
 s.homepage = 'https://github.com/fengluoxiao/SDA'
 s.source = { :git => 'https://github.com/fengluoxiao/SDA.git' }
 s.platform = :ios, '16.0'
 s.swift_version = '5.9'
 s.static_framework = true
 s.dependency 'ExpoModulesCore'
 s.source_files = '*.{h,swift}'
 s.public_header_files = 'SdaBridge.h'
 s.vendored_frameworks = 'SdaNative.xcframework'
 s.resource_bundles = { 'SdaCoreAssets' => ['Resources/hrtf', 'Resources/hrtf-dense', 'Resources/hrtf-raw', 'Resources/hrtf-dense-raw', 'Resources/rooms', 'Resources/rendering-presets.json'] }
 s.frameworks = 'AVFoundation', 'MediaPlayer', 'AudioToolbox'
 s.libraries = 'c++'
 s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES' }
end
