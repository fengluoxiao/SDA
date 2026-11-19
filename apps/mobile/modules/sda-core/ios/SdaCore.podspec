Pod::Spec.new do |s|
 s.name = 'SdaCore'
 s.version = '0.1.0'
 s.summary = 'Shared SDA Rust decoder and binaural renderer for iOS'
 s.description = s.summary
 s.license = { :type => 'GPL-3.0-or-later' }
 s.author = 'SDA contributors'
 s.homepage = 'https://github.com/fengluoxiao/SDA'
 s.source = { :git => 'https://github.com/fengluoxiao/SDA.git' }
 s.platform = :ios, '16.4'
 s.swift_version = '5.0'
 s.static_framework = true
 s.dependency 'ExpoModulesCore'
 fusion = File.exist?(File.join(__dir__, 'StandardCyborgFusion.xcframework'))
 s.source_files = ['*.{h,swift}', 'PersonalHrtfNative/**/*.{h,hpp,cpp}'] + (fusion ? ['PersonalHrtfFusion/*.swift'] : [])
 s.preserve_paths = 'PersonalHrtfNative/Generated/**/*.inc', 'PersonalHrtfNative/NOTICE.txt'
 # The Swift umbrella must resolve the same nested C ABI headers as direct
 # SDK typechecks; keep their paths and make them part of the public module.
 s.header_mappings_dir = '.'
 s.public_header_files = 'SdaBridge.h', 'PersonalHrtfNative/SdaHrtfSolver.h', 'PersonalHrtfNative/SdaHrtfMesh.h', 'PersonalHrtfNative/SdaHrtfProject.h'
 s.vendored_frameworks = ['SdaNative.xcframework'] + (fusion ? ['StandardCyborgFusion.xcframework'] : [])
 s.resource_bundles = { 'SdaCoreAssets' => ['Resources/hrtf-restored', 'Resources/rendering-presets.json', 'Resources/ios-experiment.json', 'Resources/ci-stereo-tones.m4a', 'Resources/ci-360ra.mhas', 'Licenses'] }
 s.frameworks = 'AVFoundation', 'MediaPlayer', 'AudioToolbox', 'PHASE', 'CoreMotion', 'Network', 'Accelerate', 'CoreML', 'Metal', 'ARKit', 'SceneKit', 'CoreImage'
 s.libraries = 'c++', 'z'
 s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES', 'CLANG_CXX_LANGUAGE_STANDARD' => 'c++17', 'GCC_ENABLE_CPP_EXCEPTIONS' => 'YES' }
end
