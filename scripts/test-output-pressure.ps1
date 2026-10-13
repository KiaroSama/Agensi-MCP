#requires -Version 7.0
param([switch]$Producer, [ValidateSet('stdout','stderr')][string]$Stream = 'stdout')
$ErrorActionPreference = 'Stop'
function Write-PressureLog {
    param([ValidateSet('INFO','WARNING','ERROR','DEBUG')][string]$Level, [string]$Message)
    [Console]::Error.WriteLine(('[{0:yyyy-MM-dd HH:mm:ss} UTC] [{1}] [output-pressure] {2}' -f [DateTime]::UtcNow, $Level, $Message))
}
if ($Producer) {
    # Only synthetic bytes are emitted; the caller owns the entire child process tree.
    $sink = if ($Stream -eq 'stdout') { [Console]::OpenStandardOutput() } else { [Console]::OpenStandardError() }
    $block = [byte[]]::new(8192)
    [Array]::Fill[byte]($block, [byte]120)
    for ($index = 0; $index -lt 256; $index++) { $sink.Write($block, 0, $block.Length) }
    $sink.Write([Text.Encoding]::ASCII.GetBytes('END'), 0, 3)
    $sink.Flush()
    exit 0
}
$root = Split-Path $PSScriptRoot -Parent
$name = if ($IsWindows) { 'check-run.exe' } else { 'check-run' }
$runner = Join-Path $root "target/debug/$name"
if (-not [IO.File]::Exists($runner)) { Write-PressureLog ERROR 'Build check-run before this suite.'; exit 2 }
$pwsh = (Get-Process -Id $PID).Path
$results = [Collections.Generic.List[object]]::new()
$scriptPath = $PSCommandPath
foreach ($drain in @($true, $false)) {
    foreach ($channel in @('stdout','stderr')) {
        $mode = if ($drain) { 'draining' } else { 'stalled' }
        Write-PressureLog INFO "Starting $mode consumer / $channel producer."
        $process = [Diagnostics.Process]::new()
        $memory = [IO.MemoryStream]::new()
        $watch = [Diagnostics.Stopwatch]::StartNew()
        $observed = $false
        $finished = $false
        $passed = $false
        $exitCode = $null
        $copied = $null
        $statusRead = $null
        try {
            $info = [Diagnostics.ProcessStartInfo]::new()
            $info.FileName = $runner
            $info.WorkingDirectory = $root
            $info.UseShellExecute = $false
            $info.RedirectStandardOutput = $true
            $info.RedirectStandardError = $true
            $info.CreateNoWindow = $true
            foreach ($argument in @('--wall','4','--idle','2','--',$pwsh,'-NoLogo','-NoProfile','-File',$scriptPath,'-Producer','-Stream',$channel)) {
                $info.ArgumentList.Add($argument)
            }
            $process.StartInfo = $info
            if (-not $process.Start()) { throw 'Could not start owned runner.' }
            # The status stream is drained independently; only the data consumer is stalled.
            $statusRead = $process.StandardError.ReadToEndAsync()
            if ($drain) {
                $copied = $process.StandardOutput.BaseStream.CopyToAsync($memory)
                $finished = $process.WaitForExit(7000)
                if ($finished -and $copied.Wait(2000)) {
                    $bytes = $memory.ToArray()
                    $observed = $bytes.Length -eq (2MB + 3)
                    $passed = $observed -and $process.ExitCode -eq 0 -and
                        [Text.Encoding]::ASCII.GetString($bytes, $bytes.Length - 3, 3) -ceq 'END'
                }
            } else {
                # Observe actual producer traffic before withholding reads; startup silence is not a valid reproduction.
                $first = [byte[]]::new(1)
                $read = $process.StandardOutput.BaseStream.ReadAsync($first, 0, 1)
                $observed = $read.Wait(3000) -and $read.Result -eq 1 -and $first[0] -eq 120
                $finished = $process.WaitForExit(7000)
                if ($finished) { $passed = $observed -and $process.ExitCode -ne 0 }
            }
            if ($finished) { $exitCode = $process.ExitCode }
        } catch {
            Write-PressureLog ERROR ('Fixture failed (' + $_.Exception.GetBaseException().GetType().Name + '); payload omitted.')
        } finally {
            # Closing the consumer releases a baseline blocked write before a final owned-tree kill is needed.
            try { $process.StandardOutput.Close() } catch { }
            try {
                if (-not $process.HasExited -and -not $process.WaitForExit(2000)) {
                    Write-PressureLog WARNING 'Stopping this fixture-owned process tree after its external safety deadline.'
                    $process.Kill($true)
                    if (-not $process.WaitForExit(3000)) { throw 'Owned fixture did not exit.' }
                }
                if ($null -ne $statusRead -and -not $statusRead.Wait(2000)) { $passed = $false }
            } catch { $passed = $false; Write-PressureLog ERROR 'Owned fixture cleanup failed; no success claimed.' }
            $process.Dispose()
            $memory.Dispose()
            $watch.Stop()
        }
        $results.Add([pscustomobject]@{ case = "$mode-$channel"; passed = $passed; producer_output_observed = $observed; exited_before_safety_deadline = $finished; exit_code = $exitCode; duration_ms = $watch.ElapsedMilliseconds })
        $level = if ($passed) { 'INFO' } else { 'ERROR' }
        $label = if ($passed) { 'PASS' } else { 'FAIL' }
        Write-PressureLog $level "$label $mode-$channel; synthetic payload omitted."
    }
}
[IO.Directory]::CreateDirectory((Join-Path $root '.tmp')) | Out-Null
[IO.File]::WriteAllText((Join-Path $root '.tmp/output-pressure.json'), ($results | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
$failed = @($results | Where-Object { -not $_.passed }).Count
Write-PressureLog INFO ("Completed 4 cases; failures=$failed.")
exit ([int]($failed -ne 0))
