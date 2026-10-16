#requires -Version 7.4
[CmdletBinding()]
param(
    [ValidateSet('Run','Execute','Recover','Validate','Protect','Trust','RemoveCase')][string]$Action = 'Run',
    [string]$CaseRoot,
    [string]$Binary,
    [string]$Certificate,
    [string]$StateRoot
)
# This verification owner targets hosted PowerShell 7.4+, not the production 7.0 registration floor.
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$utf8 = [Text.UTF8Encoding]::new($false, $true)
$ownerScript = $PSCommandPath
$pwsh = Join-Path $PSHOME $(if ($IsWindows) { 'pwsh.exe' } else { 'pwsh' })
if (-not [IO.File]::Exists($pwsh)) { throw 'Native PowerShell executable is absent from PSHOME.' }
$state = if ($StateRoot) { $StateRoot } else { $env:AGENSI_TLS_CI_STATE }
$log = $null
function Write-CiLog([string]$Level, [string]$Message) {
    $line = '[{0:yyyy-MM-dd HH:mm:ss} UTC] [{1}] [tls-ci] {2}' -f [DateTime]::UtcNow, $Level, $Message
    [Console]::Error.WriteLine($line)
    if ($script:log) { $script:log.WriteLine($line); $script:log.Flush() }
}
function Assert-Hosted {
    if ($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or
        -not $env:GITHUB_WORKSPACE -or [IO.Path]::GetFullPath($env:GITHUB_WORKSPACE) -ne $root) {
        throw 'TLS native fixture setup is restricted to this disposable GitHub-hosted checkout.'
    }
}
function Assert-OwnedPath([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path)
    if ($IsWindows -and $full.StartsWith('\\?\')) { $full = $full.Substring(4) }
    $prefix = [IO.Path]::GetFullPath((Join-Path $root '.tmp')) + [IO.Path]::DirectorySeparatorChar
    if (-not $full.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture state escaped project scratch.' }
    $item = $full
    while ($item) {
        if ((Test-Path -LiteralPath $item) -and ((Get-Item -LiteralPath $item -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Linked fixture state refused.' }
        $parent = Split-Path $item -Parent
        if ($parent -eq $item) { break }
        $item = $parent
    }
    return $full
}
function Invoke-Bounded([string]$Program, [string[]]$Arguments, [int]$Seconds = 10, [switch]$Isolated) {
    $process = [Diagnostics.Process]::new()
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Program
    $info.WorkingDirectory = $root
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.RedirectStandardInput = $true
    $info.StandardOutputEncoding = $utf8
    $info.StandardErrorEncoding = $utf8
    foreach ($argument in $Arguments) { $info.ArgumentList.Add($argument) }
    foreach ($name in @('AGENSI_SIGNING_PFX_B64','AGENSI_SIGNING_PASSWORD')) { $null = $info.Environment.Remove($name) }
    if ($Isolated) {
        $info.Environment.Clear()
        foreach ($name in @('PATH','SYSTEMROOT','WINDIR','GITHUB_ACTIONS','RUNNER_ENVIRONMENT','GITHUB_WORKSPACE','GITHUB_RUN_ID')) {
            $value = [Environment]::GetEnvironmentVariable($name)
            if ($null -ne $value) { $info.Environment[$name] = $value }
        }
        foreach ($name in @('HOME','USERPROFILE','APPDATA','LOCALAPPDATA','TEMP','TMP','TMPDIR')) { $info.Environment[$name] = $state }
        $info.Environment['AGENSI_TLS_CI'] = '1'
        $info.Environment['AGENSI_TLS_CI_STATE'] = $state
        $info.Environment['AGENSI_TLS_CI_SCRIPT'] = $ownerScript
        $info.Environment['AGENSI_TLS_CI_PWSH'] = $pwsh
    }
    $process.StartInfo = $info
    $processRecord = $null
    $started = $false
    try {
        $started = $process.Start()
        if (-not $started) { throw 'Fixture helper did not start.' }
        # PowerShell suppresses a reaped Unix process's StartTime getter error as null.
        # An exited child needs no recovery identity; a live child requires the real timestamp.
        if (-not $process.HasExited) {
            try {
                $startedAt = $process.get_StartTime().ToUniversalTime().Ticks
                $processRecord = Join-Path $state ('process-' + $process.Id + '.json')
                [IO.File]::WriteAllText($processRecord, (ConvertTo-Json @{ pid = $process.Id; started = $startedAt }), $utf8)
            } catch {
                if (-not $process.HasExited) { throw }
            }
        }
        $process.StandardInput.Close()
        $readers = @($process.StandardOutput, $process.StandardError)
        $buffers = @([char[]]::new(4096), [char[]]::new(4096))
        $texts = @([Text.StringBuilder]::new(), [Text.StringBuilder]::new())
        $reads = @($readers[0].ReadAsync($buffers[0], 0, 4096), $readers[1].ReadAsync($buffers[1], 0, 4096))
        $watch = [Diagnostics.Stopwatch]::StartNew()
        while ($true) {
            for ($index = 0; $index -lt 2; $index++) {
                if ($reads[$index] -and $reads[$index].IsCompleted) {
                    $count = $reads[$index].GetAwaiter().GetResult()
                    if ($texts[0].Length + $texts[1].Length + $count -gt 1MB) { throw 'Fixture output exceeded its allocation bound.' }
                    if ($count -eq 0) { $reads[$index] = $null } else {
                        $null = $texts[$index].Append($buffers[$index], 0, $count)
                        $reads[$index] = $readers[$index].ReadAsync($buffers[$index], 0, 4096)
                    }
                }
            }
            if ($process.HasExited -and -not $reads[0] -and -not $reads[1]) { break }
            if ($watch.Elapsed.TotalSeconds -ge $Seconds) { throw 'Fixture helper or pipe deadline exceeded.' }
            $pending = @($reads | Where-Object { $null -ne $_ })
            if ($pending.Count) { $null = [Threading.Tasks.Task]::WaitAny([Threading.Tasks.Task[]]$pending, 50) }
            else { $null = $process.WaitForExit(50) }
        }
        if ($process.ExitCode -ne 0) {
            $diagnostic = Join-Path $state ('helper-' + $process.Id + '.diagnostic.txt')
            # Only this synthetic fixture stage uses this owner; signing secrets are removed below.
            $failure = "[synthetic stdout]`n" + $texts[0].ToString() + "`n[synthetic stderr]`n" + $texts[1].ToString()
            [IO.File]::WriteAllText($diagnostic, $failure, $utf8)
            [Console]::Error.Write($failure)
            throw ('Fixture helper failed: ' + [IO.Path]::GetFileName($Program) + ' exit=' + $process.ExitCode)
        }
        if ($Seconds -gt 30) {
            [Console]::Out.Write($texts[0].ToString())
            [Console]::Error.Write($texts[1].ToString())
        }
        return $texts[0].ToString().Trim()
    } finally {
        if ($started -and -not $process.HasExited) {
            $process.Kill($true)
            if (-not $process.WaitForExit(3000)) { throw 'Fixture helper tree cleanup deadline.' }
        }
        $process.Dispose()
        if ($processRecord -and [IO.File]::Exists($processRecord)) { [IO.File]::Delete($processRecord) }
    }
}
function Read-Ledger {
    $path = Join-Path $state 'ledger.json'
    if (-not [IO.File]::Exists($path)) { return @{ roots = @(); rules = @() } }
    return ConvertFrom-Json -AsHashtable ([IO.File]::ReadAllText($path, $utf8))
}
function Save-Ledger($Ledger) {
    $staged = Join-Path $state ('ledger-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    [IO.File]::WriteAllText($staged, (ConvertTo-Json -Depth 6 $Ledger), $utf8)
    [IO.File]::Move($staged, (Join-Path $state 'ledger.json'), $true)
}
function Remove-CaseState {
    $ledger = Read-Ledger
    $failed = $false
    foreach ($record in @($ledger.roots)) {
        try {
            if ($IsWindows) {
                $store = [Security.Cryptography.X509Certificates.X509Store]::new('Root','CurrentUser')
                try {
                    $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
                    foreach ($cert in @($store.Certificates | Where-Object Thumbprint -CEQ $record.fingerprint)) { $store.Remove($cert) }
                    if (@($store.Certificates | Where-Object Thumbprint -CEQ $record.fingerprint).Count) { throw 'Owned root remains.' }
                } finally { $store.Dispose() }
            } elseif ($IsMacOS) {
                $existing = Invoke-Bounded '/usr/bin/security' @('find-certificate','-a','-Z','/Library/Keychains/System.keychain')
                if ($existing.Contains($record.fingerprint)) {
                    $null = Invoke-Bounded '/usr/bin/security' @('remove-trusted-cert','-d',$record.path)
                    $null = Invoke-Bounded '/usr/bin/security' @('delete-certificate','-Z',$record.fingerprint,'/Library/Keychains/System.keychain')
                }
                $remaining = Invoke-Bounded '/usr/bin/security' @('find-certificate','-a','-Z','/Library/Keychains/System.keychain')
                if ($remaining.Contains($record.fingerprint)) { throw 'Owned Apple root remains.' }
            }
        } catch { $failed = $true; Write-CiLog ERROR 'Owned CA cleanup failed; no success claimed.' }
    }
    foreach ($rule in @($ledger.rules)) {
        try {
            if ($IsWindows) {
                Get-NetFirewallRule -Name $rule -ErrorAction SilentlyContinue | Remove-NetFirewallRule -ErrorAction Stop
                if (Get-NetFirewallRule -Name $rule -ErrorAction SilentlyContinue) { throw 'Owned rule remains.' }
            }
        } catch { $failed = $true; Write-CiLog ERROR 'Owned firewall cleanup failed; no success claimed.' }
    }
    if ($failed) { throw 'Native case cleanup incomplete.' }
    Save-Ledger @{ roots = @(); rules = @() }
}
function Restore-Invocation {
    $journal = Join-Path $state 'invocation.json'
    if (-not [IO.File]::Exists($journal)) { return }
    $record = ConvertFrom-Json -AsHashtable ([IO.File]::ReadAllText($journal, $utf8))
    if ($record.restored) { return }
    $failed = $false
    foreach ($file in @(Get-ChildItem -LiteralPath $state -Filter 'process-*.json' -File)) {
        try {
            $owned = ConvertFrom-Json -AsHashtable ([IO.File]::ReadAllText($file.FullName, $utf8))
            $process = Get-Process -Id $owned.pid -ErrorAction SilentlyContinue
            if ($process -and $process.Id -ne $PID -and $process.StartTime.ToUniversalTime().Ticks -eq $owned.started) {
                $process.Kill($true)
                if (-not $process.WaitForExit(3000)) { throw 'Owned helper survived recovery.' }
            }
            [IO.File]::Delete($file.FullName)
        } catch { $failed = $true }
    }
    try { Remove-CaseState } catch { $failed = $true }
    if ($IsLinux -and $record.namespace) {
        try {
            $ip = (Get-Command ip -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
            $names = Invoke-Bounded $ip @('netns','list')
            if ($names -match ('(?m)^' + [regex]::Escape($record.namespace) + '(?:\s|$)')) {
                $pids = Invoke-Bounded $ip @('netns','pids',$record.namespace)
                foreach ($line in @($pids -split '\s+' | Where-Object { $_ })) {
                    if ($line -notmatch '^\d+$') { throw 'Invalid owned namespace PID.' }
                    $process = Get-Process -Id ([int]$line) -ErrorAction SilentlyContinue
                    if ($process) { $process.Kill($true); if (-not $process.WaitForExit(3000)) { throw 'Namespace process survived.' } }
                }
                $null = Invoke-Bounded $ip @('netns','delete',$record.namespace)
            }
        } catch { $failed = $true }
    }
    try {
        $expected = if ($IsWindows) { Join-Path $env:SYSTEMROOT 'System32/drivers/etc/hosts' } else { '/etc/hosts' }
        if ($record.hosts -ne $expected) { throw 'Recovery hosts path differs.' }
        $original = [IO.File]::ReadAllBytes((Join-Path $state 'hosts.original'))
        [IO.File]::WriteAllBytes($expected, $original)
        if (-not [Linq.Enumerable]::SequenceEqual[byte]($original,[IO.File]::ReadAllBytes($expected))) { throw 'Hosts restoration differs.' }
    } catch { $failed = $true }
    [IO.File]::WriteAllText((Join-Path $state 'cleanup.json'), (ConvertTo-Json @{ restored = -not $failed; temporary_native_state = $true }), $utf8)
    if ($failed) { throw 'CI fixture recovery incomplete.' }
    $record.restored = $true
    [IO.File]::WriteAllText($journal, (ConvertTo-Json $record), $utf8)
    Write-CiLog INFO 'Native fixture trust, network and hostname state restored.'
}
trap {
    [Console]::Error.WriteLine(('TLS bootstrap failed type={0} line={1} action={2} id={3}; values omitted.' -f $_.Exception.GetType().Name, $_.InvocationInfo.ScriptLineNumber, $Action, (($_.FullyQualifiedErrorId -split ',')[0] -replace '[^A-Za-z0-9_.-]', '_')))
    exit 1
}
Assert-Hosted
if ($Action -eq 'Run') {
    $state = Join-Path $root ('.tmp/tls-ci-' + [Guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($state) | Out-Null
    $script:state = $state
    try {
        if ($IsWindows) {
            $null = Invoke-Bounded $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',$ownerScript,'-Action','Execute','-StateRoot',$state) 165
        } else {
            $sudo = (Get-Command sudo -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
            $null = Invoke-Bounded $sudo @('-n','--preserve-env=PATH,GITHUB_ACTIONS,RUNNER_ENVIRONMENT,GITHUB_WORKSPACE,GITHUB_RUN_ID',$pwsh,'-NoLogo','-NoProfile','-NonInteractive','-File',$ownerScript,'-Action','Execute','-StateRoot',$state) 165
        }
    } finally {
        # A killed inner owner cannot run finally; the surviving outer owner replays its journal.
        if ($IsWindows) {
            $null = Invoke-Bounded $pwsh @('-NoLogo','-NoProfile','-NonInteractive','-File',$ownerScript,'-Action','Recover','-StateRoot',$state) 35
        } else {
            $null = Invoke-Bounded $sudo @('-n','--preserve-env=PATH,GITHUB_ACTIONS,RUNNER_ENVIRONMENT,GITHUB_WORKSPACE,GITHUB_RUN_ID',$pwsh,'-NoLogo','-NoProfile','-NonInteractive','-File',$ownerScript,'-Action','Recover','-StateRoot',$state) 35
        }
        Write-CiLog INFO 'Disposable native fixture owner exited with recovery checked.'
    }
    exit 0
}
$state = Assert-OwnedPath $state
if (-not [IO.Directory]::Exists($state)) { throw 'Missing fixture state root.' }
$script:state = $state
$stamp = [DateTime]::UtcNow.ToString('yyyy-MM-dd_HH-mm-ss')
$logPath = Join-Path $state ('test-tls-proxy-ci_' + $stamp + '_UTC_' + [Guid]::NewGuid().ToString('N') + '.log')
try { $log = [IO.StreamWriter]::new([IO.FileStream]::new($logPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read), $utf8) }
catch { Write-CiLog WARNING 'File logging unavailable; captured console is the fallback.' }
try {
    Write-CiLog INFO "Starting action=$Action"
    if ($Action -eq 'Execute') {
        $hosts = if ($IsWindows) { Join-Path $env:SYSTEMROOT 'System32/drivers/etc/hosts' } else { '/etc/hosts' }
        $backup = Join-Path $state 'hosts.original'
        $original = [IO.File]::ReadAllBytes($hosts)
        [IO.File]::WriteAllBytes($backup, $original)
        $namespace = 'agensi-' + [Guid]::NewGuid().ToString('N')
        # Write intent before any privileged mutation so the outer owner can recover a killed run.
        [IO.File]::WriteAllText((Join-Path $state 'invocation.json'), (ConvertTo-Json @{ hosts = $hosts; namespace = $(if ($IsLinux) { $namespace } else { $null }); restored = $false }), $utf8)
        try {
            $text = $utf8.GetString($original)
            if ($text -match '(?m)^\s*[^#\r\n]+\s+(?:mcp\.agensi\.io|auth\.agensi-audit\.test)(?:\s|$)') { throw 'Preexisting observer hostname mapping refused.' }
            [IO.File]::WriteAllText($hosts, $text + "`n127.0.0.1 mcp.agensi.io auth.agensi-audit.test`n", $utf8)
            $env:AGENSI_TLS_CI = '1'
            $env:AGENSI_TLS_CI_STATE = $state
            $env:AGENSI_TLS_CI_SCRIPT = $ownerScript
            $env:AGENSI_TLS_CI_PWSH = $pwsh
            Save-Ledger @{ roots = @(); rules = @() }
            $suffix = if ($IsWindows) { '.exe' } else { '' }
            $runner = Join-Path $root ".tmp/check-run$suffix"
            if (-not [IO.File]::Exists($runner)) { throw 'Prebuilt copied check-run is required.' }
            $targets = @(Get-ChildItem -LiteralPath (Join-Path $root 'target/debug/deps') -File | Where-Object { $_.Name -match ('^audit_blackbox-[0-9a-f]+' + [regex]::Escape($suffix) + '$') })
            if ($targets.Count -ne 1) { throw 'Expected exactly one prebuilt native audit harness.' }
            $arguments = @('--wall','120','--idle','20','--',$targets[0].FullName,'--source-root',$root,'--phase=tls-proxy','--output',(Join-Path $root '.tmp/tls-proxy-results.json'))
            if ($IsLinux) {
                $ip = (Get-Command ip -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
                $null = Invoke-Bounded $ip @('netns','add',$namespace)
                $null = Invoke-Bounded $ip @('-n',$namespace,'link','set','lo','up')
                $null = Invoke-Bounded $ip (@('netns','exec',$namespace,$runner) + $arguments) 130 -Isolated
            } elseif ($IsMacOS) {
                if (-not [IO.File]::Exists('/usr/bin/sandbox-exec')) { throw 'Required macOS native sandbox is unavailable.' }
                $profile = Join-Path $state 'loopback.sb'
                [IO.File]::WriteAllText($profile, '(version 1)(allow default)(deny network*)(allow network-outbound (remote ip "localhost:*"))(allow network-inbound (local ip "localhost:*"))(allow network-bind (local ip "localhost:*"))(allow network-outbound (remote unix-socket))', $utf8)
                $null = Invoke-Bounded '/usr/bin/sandbox-exec' (@('-f',$profile,$runner) + $arguments) 130 -Isolated
            } elseif ($IsWindows) {
                $null = Invoke-Bounded $runner $arguments 130 -Isolated
            } else { throw 'Unsupported native platform.' }
        } finally {
            Restore-Invocation
        }
    } elseif ($Action -eq 'Recover') { Restore-Invocation }
    elseif ($Action -eq 'Validate') {
        foreach ($hostname in @('mcp.agensi.io','auth.agensi-audit.test')) {
            $lookup = [Net.Dns]::GetHostAddressesAsync($hostname)
            if (-not $lookup.Wait(3000)) { throw 'Observer hostname resolution deadline.' }
            $addresses = @($lookup.Result)
            if (-not $addresses.Count -or @($addresses | Where-Object { $_.ToString() -ne '127.0.0.1' }).Count) { throw 'Observer hostname escaped IPv4 loopback.' }
        }
        if ($IsLinux) {
            $ip = (Get-Command ip -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
            if ((Invoke-Bounded $ip @('-o','link','show')) -match '(?m)^\d+: (?!lo[: ])') { throw 'Namespace contains nonloopback interface.' }
        }
    } elseif ($Action -eq 'RemoveCase') { Remove-CaseState }
    elseif ($Action -eq 'Protect') {
        $null = Assert-OwnedPath $CaseRoot
        $Binary = Assert-OwnedPath $Binary
        if (-not [IO.File]::Exists($Binary)) { throw 'Copied CLI is absent.' }
        if ($IsWindows) {
            $name = 'AgensiTlsFixture-' + [Guid]::NewGuid().ToString('N')
            $ledger = Read-Ledger
            $ledger.rules += $name
            Save-Ledger $ledger
            if (@(Get-NetFirewallProfile | Where-Object Enabled -NE 'True').Count) { throw 'Native firewall profiles must already be enabled.' }
            $ranges = @('0.0.0.0-126.255.255.255','128.0.0.0-255.255.255.255','::','::2-ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff')
            New-NetFirewallRule -Name $name -DisplayName $name -Direction Outbound -Action Block -Program $Binary -RemoteAddress $ranges -Profile Any -Enabled True | Out-Null
            $rule = Get-NetFirewallRule -Name $name
            $observedRanges = @((Get-NetFirewallAddressFilter -AssociatedNetFirewallRule $rule).RemoteAddress)
            if ($rule.Action -ne 'Block' -or $rule.Direction -ne 'Outbound' -or $rule.Enabled -ne 'True' -or
                (Get-NetFirewallApplicationFilter -AssociatedNetFirewallRule $rule).Program -ne $Binary -or
                @(Compare-Object ($ranges | Sort-Object) ($observedRanges | Sort-Object)).Count) { throw 'Exact copied CLI firewall restriction was not applied.' }
        }
    } elseif ($Action -eq 'Trust') {
        $null = Assert-OwnedPath $CaseRoot
        $Certificate = Assert-OwnedPath $Certificate
        $cert = [Security.Cryptography.X509Certificates.X509Certificate2]::CreateFromPem([IO.File]::ReadAllText($Certificate, $utf8))
        try {
            $fingerprint = $cert.Thumbprint
            $owned = Join-Path $state ($fingerprint + '.pem')
            [IO.File]::WriteAllText($owned, $cert.ExportCertificatePem(), $utf8)
            if ($IsWindows) {
                $store = [Security.Cryptography.X509Certificates.X509Store]::new('Root','CurrentUser')
                try {
                    $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
                    if (@($store.Certificates | Where-Object Thumbprint -CEQ $fingerprint).Count) { throw 'Preexisting root collision refused.' }
                    $ledger = Read-Ledger
                    $ledger.roots += @{ fingerprint = $fingerprint; path = $owned }
                    Save-Ledger $ledger
                    $store.Add($cert)
                    if (@($store.Certificates | Where-Object Thumbprint -CEQ $fingerprint).Count -ne 1) { throw 'Owned root import not observed.' }
                } finally { $store.Dispose() }
            } elseif ($IsMacOS) {
                $existing = Invoke-Bounded '/usr/bin/security' @('find-certificate','-a','-Z','/Library/Keychains/System.keychain')
                # Trust settings reference an installed certificate; refuse an existing exact random-CA fingerprint.
                # dump-trust-settings returns nonzero for the normal empty-admin-settings state.
                if ($existing.Contains($fingerprint)) { throw 'Preexisting native CA/trust collision refused.' }
                $ledger = Read-Ledger
                $ledger.roots += @{ fingerprint = $fingerprint; path = $owned }
                Save-Ledger $ledger
                $null = Invoke-Bounded '/usr/bin/security' @('add-trusted-cert','-d','-r','trustRoot','-p','ssl','-k','/Library/Keychains/System.keychain',$owned)
                $observed = Invoke-Bounded '/usr/bin/security' @('find-certificate','-a','-Z','/Library/Keychains/System.keychain')
                if (-not $observed.Contains($fingerprint)) { throw 'Owned root import not observed.' }
            }
        } finally { $cert.Dispose() }
    }
    Write-CiLog INFO "Completed action=$Action"
} catch {
    Write-CiLog ERROR ('Native fixture action failed type={0} line={1} action={2} id={3}; payload omitted.' -f $_.Exception.GetType().Name, $_.InvocationInfo.ScriptLineNumber, $Action, (($_.FullyQualifiedErrorId -split ',')[0] -replace '[^A-Za-z0-9_.-]', '_'))
    exit 1
} finally { if ($log) { $log.Dispose() } }
