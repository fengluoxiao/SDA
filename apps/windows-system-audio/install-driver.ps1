#Requires -RunAsAdministrator
param(
    [string]$Package = "$PSScriptRoot/../../tmp/sda-system-audio-package",
    [string]$DevCon = "${env:ProgramFiles(x86)}/Windows Kits/10/Tools/10.0.26100.0/x64/devcon.exe",
    [switch]$Remove
)
$ErrorActionPreference = 'Stop'
if (!(Test-Path -LiteralPath $DevCon -PathType Leaf)) { throw 'Specify the WDK x64 devcon.exe using -DevCon.' }
if ($Remove) {
    & $DevCon remove 'Root\SdaSystemAudio'
} else {
    $resolved = (Resolve-Path -LiteralPath $Package).Path
    foreach ($file in @('SdaSystemAudio.sys', 'SdaSystemAudio.cat')) {
        $signature = Get-AuthenticodeSignature (Join-Path $resolved $file)
        if ($signature.Status -ne 'Valid') { throw "$file does not have a trusted signature. Configure a test environment or use a properly signed package first." }
    }
    # Register the root-enumerated device, not just its package in DriverStore.
    # No /r: never restart the user's computer automatically.
    & $DevCon install (Join-Path $resolved 'SdaSystemAudio.inf') 'Root\SdaSystemAudio'
}
if ($LASTEXITCODE -eq 1) { Write-Output 'Windows requests a restart to finish the device change.' }
elseif ($LASTEXITCODE -ne 0) { throw "Device operation failed: $LASTEXITCODE" }
else { Write-Output 'Device operation completed. Check the device status before claiming audio input is available.' }
