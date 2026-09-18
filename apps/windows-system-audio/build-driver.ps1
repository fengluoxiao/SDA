param([string]$SdkVersion = '10.0.26100.0', [switch]$TestSign)
$ErrorActionPreference = 'Stop'
$sdaRoot = (Resolve-Path "$PSScriptRoot/../..").Path
$vswhere = "${env:ProgramFiles(x86)}/Microsoft Visual Studio/Installer/vswhere.exe"
$vs = & $vswhere -latest -products '*' -requires Microsoft.Component.MSBuild -property installationPath
if (!$vs) { throw 'Visual Studio C++ Build Tools and WDK are required.' }
$msbuild = Join-Path $vs 'MSBuild/Current/Bin/amd64/MSBuild.exe'
& python "$PSScriptRoot/prepare-driver.py"
if ($LASTEXITCODE) { throw 'Driver source preparation failed.' }
$base = Join-Path $sdaRoot 'tmp/sda-system-audio-driver/audio/sysvad'
foreach ($project in @('EndpointsCommon/EndpointsCommon.vcxproj', 'TabletAudioSample/TabletAudioSample.vcxproj')) {
    & $msbuild (Join-Path $base $project) /p:Configuration=Release /p:Platform=x64 "/p:WindowsTargetPlatformVersion=$SdkVersion" /m /v:minimal
    if ($LASTEXITCODE) { throw "WDK build failed: $project" }
}
$package = Join-Path $sdaRoot 'tmp/sda-system-audio-package'
New-Item -ItemType Directory -Force $package | Out-Null
Copy-Item -LiteralPath (Join-Path $base 'TabletAudioSample/x64/Release/SdaSystemAudio.sys') -Destination $package
Copy-Item -LiteralPath "$PSScriptRoot/driver/SdaSystemAudio.inf" -Destination $package
$kit = "${env:ProgramFiles(x86)}/Windows Kits/10"
& "$kit/Tools/$SdkVersion/x64/infverif.exe" /v (Join-Path $package 'SdaSystemAudio.inf')
if ($LASTEXITCODE) { throw 'INF validation failed.' }
& "$kit/bin/$SdkVersion/x86/Inf2Cat.exe" "/driver:$package" /os:10_NI_X64 /uselocaltime
if ($LASTEXITCODE) { throw 'Catalog generation failed.' }
if ($TestSign) {
    $signer = (Get-AuthenticodeSignature (Join-Path $package 'SdaSystemAudio.sys')).SignerCertificate
    if (!$signer -or $signer.Subject -notmatch '^CN="?WDKTestCert ') { throw 'Only the WDK development test certificate may be used here.' }
    & "$kit/bin/$SdkVersion/x64/signtool.exe" sign /fd SHA256 /s My /sha1 $signer.Thumbprint (Join-Path $package 'SdaSystemAudio.cat')
    if ($LASTEXITCODE) { throw 'Development catalog signing failed.' }
}
Write-Output "Built $package. Administrator installation is separate; no device or boot settings were changed."
