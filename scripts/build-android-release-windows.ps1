$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot/..").Path
Set-Location $root
$out = "$root/android-artifacts"
New-Item -ItemType Directory -Path $out -Force | Out-Null
$env:ANDROID_SDK_ROOT = $env:ANDROID_HOME
$env:ANDROID_NDK_HOME = "$env:ANDROID_HOME/ndk/27.1.12297006"
$bin = "$env:ANDROID_NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64/bin"
$env:SDA_NODE = (Get-Command node).Source
$env:MACINDECODE_AC4_SPEC_DIR = "$root/tmp/MacinDecode-AC4-Core/spec"
$env:PATH = "$bin;$env:PATH"
$env:SDA_ANDROID_RELEASE_STORE_FILE = "$env:RUNNER_TEMP/sda-official-release.p12"
try {
 foreach ($name in @('SDA_ANDROID_KEYSTORE_BASE64','SDA_ANDROID_RELEASE_STORE_PASSWORD','SDA_ANDROID_RELEASE_KEY_ALIAS','SDA_ANDROID_RELEASE_KEY_PASSWORD')) {
  if (-not [Environment]::GetEnvironmentVariable($name)) { throw "Missing signing secret: $name" }
 }
 [IO.File]::WriteAllBytes($env:SDA_ANDROID_RELEASE_STORE_FILE, [Convert]::FromBase64String($env:SDA_ANDROID_KEYSTORE_BASE64))
 foreach ($entry in @(@('aarch64-linux-android','arm64-v8a'),@('x86_64-linux-android','x86_64'))) {
  $target = $entry[0]; $abi = $entry[1]; $suffix = $target.Replace('-','_')
  [Environment]::SetEnvironmentVariable("CC_$suffix", "$bin/${target}26-clang.cmd")
  [Environment]::SetEnvironmentVariable("CXX_$suffix", "$bin/${target}26-clang++.cmd")
  [Environment]::SetEnvironmentVariable("AR_$suffix", "$bin/llvm-ar.exe")
  $upper = $suffix.ToUpperInvariant()
  [Environment]::SetEnvironmentVariable("CARGO_TARGET_${upper}_LINKER", "$bin/clang.exe")
  [Environment]::SetEnvironmentVariable("CARGO_TARGET_${upper}_RUSTFLAGS", "-C link-arg=--target=${target}26")
  & cargo build --manifest-path crates/sda-native/Cargo.toml --target $target --release --no-default-features --locked -j 3 *> "$out/rust-$target.log"
  if ($LASTEXITCODE -ne 0) { Get-Content "$out/rust-$target.log" -Tail 50; throw "Native build failed: $target" }
  $dest = "$root/apps/mobile/android/app/src/main/jniLibs/$abi"
  New-Item -ItemType Directory -Path $dest -Force | Out-Null
  Copy-Item -LiteralPath "$env:CARGO_TARGET_DIR/$target/release/libsda_native.so" -Destination "$dest/libsda_native.so" -Force
 }
 python scripts/record-android-release.py stage
 if ($LASTEXITCODE -ne 0) { throw 'Native provenance failed' }
 $jvmTemp = "$env:SystemDrive/sda-jvm"
 New-Item -ItemType Directory -Path $jvmTemp -Force | Out-Null
 $env:JAVA_TOOL_OPTIONS = "-Djava.net.preferIPv4Stack=true -Djdk.net.unixdomain.tmpdir=$jvmTemp -Djava.io.tmpdir=$jvmTemp"
 $sdkPath = $env:ANDROID_HOME.Replace('\','/')
 [IO.File]::WriteAllText("$root/apps/mobile/android/local.properties", "sdk.dir=$sdkPath`n")
 Push-Location apps/mobile/android
 try {
  & ./gradlew.bat :app:assembleRelease --no-daemon --stacktrace --max-workers=2 --console=plain '-PsdaOfficialRelease=true' '-PreactNativeArchitectures=arm64-v8a,x86_64' '-PsdaVendorAudio=false' '-PsdaVendorAudioLegacyOnly=false' "-Porg.gradle.java.installations.paths=$env:JAVA_HOME" '-Pkotlin.incremental=false' '-Pkotlin.compiler.execution.strategy=in-process' '-Dorg.gradle.jvmargs=-Xmx4096m -XX:MaxMetaspaceSize=1024m' *> "$out/gradle.log"
  if ($LASTEXITCODE -ne 0) { Get-Content "$out/gradle.log" -Tail 70; throw 'Gradle release build failed' }
 } finally { Pop-Location }
 $version = (Get-Content apps/mobile/app.json -Raw | ConvertFrom-Json).expo.version
 $apk = "$out/SDA-$version-Android-universal.apk"
 Copy-Item -LiteralPath apps/mobile/android/app/build/outputs/apk/release/app-release.apk -Destination $apk
 & "$env:ANDROID_HOME/build-tools/36.0.0/apksigner.bat" verify --verbose --print-certs $apk > "$out/signature.txt"
 if ($LASTEXITCODE -ne 0) { throw 'APK signature validation failed' }
 & "$env:ANDROID_HOME/build-tools/36.0.0/zipalign.exe" -c -P 16 -v 4 $apk > "$out/zipalign.txt"
 if ($LASTEXITCODE -ne 0) { throw 'APK 16 KiB ZIP alignment failed' }
 & "$env:ANDROID_HOME/build-tools/36.0.0/aapt.exe" dump badging $apk > "$out/badging.txt"
 if ($LASTEXITCODE -ne 0) { throw 'APK metadata validation failed' }
 python scripts/record-android-release.py verify
 if ($LASTEXITCODE -ne 0) { throw 'APK source/ABI/signing verification failed' }
} finally {
 if (Test-Path -LiteralPath $env:SDA_ANDROID_RELEASE_STORE_FILE) { Remove-Item -LiteralPath $env:SDA_ANDROID_RELEASE_STORE_FILE -Force }
}
