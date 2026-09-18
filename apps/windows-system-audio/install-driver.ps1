param(
    [string]$Package = "$PSScriptRoot/../../tmp/sda-system-audio-package",
    [string]$DevCon = "${env:ProgramFiles(x86)}/Windows Kits/10/Tools/10.0.26100.0/x64/devcon.exe",
    [switch]$Remove
)
$ErrorActionPreference = 'Stop'
$administrator = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (!$administrator) {
    foreach ($value in @($PSCommandPath, $Package, $DevCon)) {
        if ($value.Contains('"')) { throw 'Paths must not contain double quotes.' }
    }
    $arguments = @('-NoProfile', '-File', "`"$PSCommandPath`"", '-Package', "`"$Package`"", '-DevCon', "`"$DevCon`"")
    if ($Remove) { $arguments += '-Remove' }
    $process = Start-Process -FilePath "$env:SystemRoot/System32/WindowsPowerShell/v1.0/powershell.exe" -Verb RunAs -WindowStyle Hidden -ArgumentList $arguments -PassThru -Wait
    if ($process.ExitCode -ne 0) { throw "Elevated installation failed (exit $($process.ExitCode))." }
    return
}
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
    $existing = @(Get-CimInstance Win32_PnPEntity | Where-Object { $_.HardwareID -contains 'ROOT\SdaSystemAudio' })
    if ($existing.Count) {
        & $DevCon update (Join-Path $resolved 'SdaSystemAudio.inf') 'Root\SdaSystemAudio'
    } else {
        & $DevCon install (Join-Path $resolved 'SdaSystemAudio.inf') 'Root\SdaSystemAudio'
    }
}
if ($LASTEXITCODE -eq 1) { Write-Output 'Windows requests a restart to finish the device change.' }
elseif ($LASTEXITCODE -ne 0) { throw "Device operation failed: $LASTEXITCODE" }
else {
    if (!$Remove) {
        $devices = @(Get-CimInstance Win32_PnPEntity | Where-Object { $_.HardwareID -contains 'ROOT\SdaSystemAudio' })
        if (!$devices.Count) { throw 'Driver package was processed but no SDA device exists.' }
        foreach ($device in $devices) {
            if ($device.ConfigManagerErrorCode -ne 0) { throw "SDA device failed to start (Code $($device.ConfigManagerErrorCode), $($device.PNPDeviceID))." }
        }
    }
    Write-Output 'Device operation completed; PnP device status is healthy.'
}
