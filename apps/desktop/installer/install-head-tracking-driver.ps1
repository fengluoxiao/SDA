param(
    [switch]$EnableTestSigning,
    [switch]$InstallDriver
)

$ErrorActionPreference = 'Stop'
$expectedCertificateSha256 = '887FBB9BFF2D202DA0E0D828FEF7C0CA8B422193424F8C658E6ADB50A37EBFB5'

function Test-IsAdministrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

if (-not $EnableTestSigning -and -not $InstallDriver) {
    exit 0
}

if (-not (Test-IsAdministrator)) {
    $elevatedArguments = @(
        '-NoLogo',
        '-NoProfile',
        '-NonInteractive',
        '-ExecutionPolicy', 'Bypass',
        '-File', ('"{0}"' -f $PSCommandPath)
    )
    if ($EnableTestSigning) { $elevatedArguments += '-EnableTestSigning' }
    if ($InstallDriver) { $elevatedArguments += '-InstallDriver' }

    try {
        $process = Start-Process -FilePath 'powershell.exe' `
            -ArgumentList $elevatedArguments `
            -Verb RunAs `
            -WindowStyle Hidden `
            -Wait `
            -PassThru
        exit $process.ExitCode
    }
    catch {
        Write-Error "Administrator approval was cancelled or failed: $($_.Exception.Message)"
        exit 1223
    }
}

$logDirectory = Join-Path $env:ProgramData 'SDA\Logs'
$logPath = Join-Path $logDirectory 'head-tracking-driver-install.log'
$transcriptStarted = $false

try {
    New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null
    Start-Transcript -Path $logPath -Force | Out-Null
    $transcriptStarted = $true

    if ($EnableTestSigning) {
        Write-Output 'Enabling Windows TestSigning. A restart is required before this takes effect.'
        & bcdedit.exe /set testsigning on
        if ($LASTEXITCODE -ne 0) {
            throw "Enabling TestSigning failed with exit code $LASTEXITCODE. Secure Boot policy may block this setting."
        }
    }

    if ($InstallDriver) {
        $certificatePath = Join-Path $PSScriptRoot 'SdaAirPodsL2cap.cer'
        $driverPath = Join-Path $PSScriptRoot 'SdaAirPodsL2cap.inf'
        foreach ($requiredPath in @($certificatePath, $driverPath)) {
            if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
                throw "Required driver file is missing: $requiredPath"
            }
        }

        $certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new($certificatePath)
        $actualSha256 = $certificate.GetCertHashString(
            [Security.Cryptography.HashAlgorithmName]::SHA256
        ).ToUpperInvariant()
        if ($actualSha256 -ne $expectedCertificateSha256) {
            throw "Certificate SHA-256 mismatch. Expected $expectedCertificateSha256, got $actualSha256."
        }

        Write-Output "Verified driver certificate SHA-256: $actualSha256"
        & certutil.exe -addstore -f Root $certificatePath
        if ($LASTEXITCODE -ne 0) { throw "Root certificate import failed with exit code $LASTEXITCODE." }
        & certutil.exe -addstore -f TrustedPublisher $certificatePath
        if ($LASTEXITCODE -ne 0) { throw "TrustedPublisher certificate import failed with exit code $LASTEXITCODE." }

        & pnputil.exe /add-driver $driverPath /install
        if ($LASTEXITCODE -ne 0) { throw "Driver package installation failed with exit code $LASTEXITCODE." }
    }

    Write-Output 'Selected SDA head-tracking setup actions completed. Restart Windows before using the driver.'
    exit 0
}
catch {
    Write-Error $_
    exit 1
}
finally {
    if ($transcriptStarted) {
        Stop-Transcript | Out-Null
    }
}
