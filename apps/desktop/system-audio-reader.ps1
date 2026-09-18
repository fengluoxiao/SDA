param([string]$PipeName, [string]$Token, [string]$CapturePath)
$ErrorActionPreference = 'Stop'
$pipe = [IO.Pipes.NamedPipeClientStream]::new('.', $PipeName, [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
$child = $null
try {
    $pipe.Connect(15000)
    $auth = [Text.Encoding]::ASCII.GetBytes($Token + "`n")
    $pipe.Write($auth, 0, $auth.Length)
    $pipe.Flush()
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $CapturePath
    $info.Arguments = '86400 --return'
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $child = [Diagnostics.Process]::Start($info)
    $errors = $child.StandardError.ReadToEndAsync()
    $copy = $child.StandardOutput.BaseStream.CopyToAsync($pipe)
    $control = $pipe.CopyToAsync($child.StandardInput.BaseStream)
    [Threading.Tasks.Task]::WaitAny([Threading.Tasks.Task[]]@($copy, $control)) | Out-Null
} finally {
    if ($child) { if (!$child.HasExited) { $child.Kill() }; $child.Dispose() }
    $pipe.Dispose()
}
