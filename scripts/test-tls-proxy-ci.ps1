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
function Invoke-Bounded([string]$Program, [string[]]$Arguments, [int]$Seconds = 10, [switch]$Isolated, [string]$InputText, [switch]$PreserveOutput) {
    if ($utf8.GetByteCount([string]$InputText) -gt 32768) { throw 'Fixture helper stdin exceeded its allocation bound.' }
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
    $info.StandardInputEncoding = $utf8
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
        $watch = [Diagnostics.Stopwatch]::StartNew()
        $write = if ($InputText) { $process.StandardInput.WriteAsync($InputText) } else { $null }
        if (-not $write) { $process.StandardInput.Close() }
        $readers = @($process.StandardOutput, $process.StandardError)
        $buffers = @([char[]]::new(4096), [char[]]::new(4096))
        $texts = @([Text.StringBuilder]::new(), [Text.StringBuilder]::new())
        $reads = @($readers[0].ReadAsync($buffers[0], 0, 4096), $readers[1].ReadAsync($buffers[1], 0, 4096))
        while ($true) {
            if ($write -and $write.IsCompleted) { $write.GetAwaiter().GetResult(); $write = $null; $process.StandardInput.Close() }
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
            if ($process.HasExited -and -not $reads[0] -and -not $reads[1] -and -not $write) { break }
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
        if ($PreserveOutput) { return $texts[0].ToString() }
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
function Convert-PlistPolicy([string]$Text) {
    $settings = [Xml.XmlReaderSettings]::new()
    $settings.DtdProcessing = [Xml.DtdProcessing]::Ignore
    $settings.XmlResolver = $null
    $reader = [Xml.XmlReader]::Create([IO.StringReader]::new($Text), $settings)
    $document = [Xml.XmlDocument]::new()
    try { $document.Load($reader) } finally { $reader.Dispose() }
    $dictionary = $document.SelectSingleNode('/plist/dict')
    if (-not $dictionary) { throw 'Authorization definition is not a plist dictionary.' }
    # authd regenerates these bookkeeping fields on write; no policy field is excluded.
    foreach ($name in @('created','modified')) {
        $key = $dictionary.SelectSingleNode("key[text()='$name']")
        if ($key) { $null = $dictionary.RemoveChild($key.NextSibling); $null = $dictionary.RemoveChild($key) }
    }
    function Convert-Node($Node) {
        $children = @($Node.ChildNodes | Where-Object NodeType -EQ ([Xml.XmlNodeType]::Element))
        if ($Node.Name -eq 'dict') {
            if ($children.Count % 2) { throw 'Malformed authorization dictionary.' }
            $pairs = [Collections.Generic.SortedDictionary[string,string]]::new([StringComparer]::Ordinal)
            for ($i = 0; $i -lt $children.Count; $i += 2) {
                $key = $children[$i].InnerText
                if ($children[$i].Name -ne 'key' -or $pairs.ContainsKey($key)) { throw 'Invalid or duplicate authorization key.' }
                $pairs[$key] = Convert-Node $children[$i + 1]
            }
            return 'dict:' + (ConvertTo-Json -Compress -Depth 30 @($pairs.Keys | ForEach-Object { @($_,$pairs[$_]) }))
        }
        if ($Node.Name -eq 'array') { return 'array:' + (ConvertTo-Json -Compress -Depth 30 @($children | ForEach-Object { Convert-Node $_ })) }
        if ($Node.Name -notin @('string','integer','real','date','data','true','false')) { throw 'Unsupported authorization plist value.' }
        return ConvertTo-Json -Compress @($Node.Name,$Node.InnerText)
    }
    return Convert-Node $dictionary
}
function Test-PlistPolicy {
    $base = '<plist><dict><key>Case</key><string>A</string><key>case</key><string>B</string><key>modified</key><real>1</real><key>rule</key><array><string>one</string><string>two</string></array></dict></plist>'
    $policy = Convert-PlistPolicy $base
    if ($policy -cne (Convert-PlistPolicy ($base.Replace('<real>1</real>','<real>2</real>')))) { throw 'Authorization bookkeeping comparison control failed.' }
    foreach ($changed in @(
        $base.Replace('<string>B</string>','<string>C</string>'),
        $base.Replace('<string>one</string><string>two</string>','<string>two</string><string>one</string>'),
        $base.Replace('<string>B</string>','<integer>1</integer>')
    )) { if ($policy -ceq (Convert-PlistPolicy $changed)) { throw 'Authorization policy mutation comparison control failed.' } }
    $rejected = $false
    try { $null = Convert-PlistPolicy ($base.Replace('<key>case</key>','<key>Case</key>')) } catch { $rejected = $true }
    if (-not $rejected) { throw 'Duplicate authorization key control failed.' }
}
function Read-MacCleanupRight {
    return Invoke-Bounded '/usr/bin/security' @('authorizationdb','read','com.apple.trust-settings.admin') -PreserveOutput
}
function Restore-MacCleanupRight {
    $pending = Join-Path $state 'mac-authorization.pending'
    if (-not $IsMacOS -or -not [IO.File]::Exists($pending)) { return }
    Write-CiLog INFO 'Cleanup stage=mac-restore-authorization-policy'
    $original = [IO.File]::ReadAllText((Join-Path $state 'mac-authorization.original.plist'), $utf8)
    $expected = Convert-PlistPolicy $original
    $null = Invoke-Bounded '/usr/bin/security' @('authorizationdb','write','com.apple.trust-settings.admin') -InputText $original
    if ((Convert-PlistPolicy (Read-MacCleanupRight)) -cne $expected) { throw 'Original administrative trust authorization policy was not restored.' }
    [IO.File]::Delete($pending)
    Write-CiLog INFO 'Original authorization policy restored; native bookkeeping timestamps excluded.'
}
function Enable-MacCleanupRight {
    if (-not $IsMacOS -or (Invoke-Bounded '/usr/bin/id' @('-u')) -cne '0') { throw 'Administrative cleanup authorization requires the hosted root fixture owner.' }
    Restore-MacCleanupRight
    $snapshot = Join-Path $state 'mac-authorization.original.plist'
    $original = Read-MacCleanupRight
    if ($utf8.GetByteCount($original) -gt 32768) { throw 'Original authorization plist exceeds the bounded restore input.' }
    $null = Convert-PlistPolicy $original
    if ([IO.File]::Exists($snapshot)) {
        if ((Convert-PlistPolicy ([IO.File]::ReadAllText($snapshot,$utf8))) -cne (Convert-PlistPolicy $original)) { throw 'Administrative trust authorization changed outside the fixture.' }
    } else { [IO.File]::WriteAllText($snapshot,$original,$utf8) }
    # This marker is durable before the native write; outer recovery restores after a killed helper.
    [IO.File]::WriteAllText((Join-Path $state 'mac-authorization.pending'), 'restore-original-policy', $utf8)
    Write-CiLog INFO 'Cleanup stage=mac-enable-root-only-authorization'
    $null = Invoke-Bounded '/usr/bin/security' @('authorizationdb','write','com.apple.trust-settings.admin','is-root')
    $observed = Read-MacCleanupRight
    # Native exports add defaults; inspect the delegated rule, never accept class=allow.
    $xml = [Xml.XmlDocument]::new()
    $xml.XmlResolver = $null
    $xml.LoadXml($observed)
    $class = $xml.SelectSingleNode('/plist/dict/key[text()="class"]/following-sibling::*[1]')
    $rules = $xml.SelectSingleNode('/plist/dict/key[text()="rule"]/following-sibling::*[1]')
    if ($class.InnerText -cne 'rule' -or $rules.Name -cne 'array' -or $rules.ChildNodes.Count -ne 1 -or $rules.FirstChild.InnerText -cne 'is-root') { throw 'Root-only cleanup authorization delegation was not observed.' }
}
function Remove-CaseState {
    try {
        if ($IsMacOS -and @( (Read-Ledger).roots ).Count) { Enable-MacCleanupRight }
        Remove-CaseStateCore
    } finally { Restore-MacCleanupRight }
}
function Remove-CaseStateCore {
    $cleanupStage = 'ledger'
    $ledger = Read-Ledger
    $failed = $false
    foreach ($record in @($ledger.roots)) {
        try {
            if ($IsWindows) {
                $store = [Security.Cryptography.X509Certificates.X509Store]::new('Root','LocalMachine')
                try {
                    $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
                    foreach ($cert in @($store.Certificates | Where-Object Thumbprint -CEQ $record.fingerprint)) { $store.Remove($cert) }
                    if (@($store.Certificates | Where-Object Thumbprint -CEQ $record.fingerprint).Count) { throw 'Owned root remains.' }
                } finally { $store.Dispose() }
            } elseif ($IsMacOS) {
                $cleanupStage = 'mac-find-owned-root'
                Write-CiLog INFO "Cleanup stage=$cleanupStage"
                $existing = Invoke-Bounded '/usr/bin/security' @('find-certificate','-a','-Z','/Library/Keychains/System.keychain')
                if ($existing.Contains($record.fingerprint)) {
                    $cleanupStage = 'mac-remove-owned-trust'
                    Write-CiLog INFO "Cleanup stage=$cleanupStage"
                    $null = Invoke-Bounded '/usr/bin/security' @('remove-trusted-cert','-d',$record.path)
                    $cleanupStage = 'mac-delete-owned-root'
                    Write-CiLog INFO "Cleanup stage=$cleanupStage"
                    $null = Invoke-Bounded '/usr/bin/security' @('delete-certificate','-Z',$record.fingerprint,'/Library/Keychains/System.keychain')
                }
                $cleanupStage = 'mac-verify-root-absent'
                Write-CiLog INFO "Cleanup stage=$cleanupStage"
                $remaining = Invoke-Bounded '/usr/bin/security' @('find-certificate','-a','-Z','/Library/Keychains/System.keychain')
                if ($remaining.Contains($record.fingerprint)) { throw 'Owned Apple root remains.' }
            }
        } catch {
            $failed = $true
            Write-CiLog ERROR ('Owned CA cleanup failed stage={0} type={1} line={2}; no success claimed.' -f $cleanupStage, $_.Exception.GetType().Name, $_.InvocationInfo.ScriptLineNumber)
        }
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
        Test-PlistPolicy
        $hosts = if ($IsWindows) { Join-Path $env:SYSTEMROOT 'System32/drivers/etc/hosts' } else { '/etc/hosts' }
        $backup = Join-Path $state 'hosts.original'
        $original = [IO.File]::ReadAllBytes($hosts)
        [IO.File]::WriteAllBytes($backup, $original)
        $namespace = 'agensi-' + [Guid]::NewGuid().ToString('N')
        # Write intent before any privileged mutation so the outer owner can recover a killed run.
        [IO.File]::WriteAllText((Join-Path $state 'invocation.json'), (ConvertTo-Json @{ hosts = $hosts; namespace = $(if ($IsLinux) { $namespace } else { $null }); restored = $false }), $utf8)
        try {
            if ($IsMacOS) {
                $controlled = $false
                $armed = $false
                try {
                    Enable-MacCleanupRight
                    $armed = $true
                    throw [OperationCanceledException]::new('Synthetic cleanup restoration control.')
                } catch [OperationCanceledException] {
                    if (-not $armed) { throw }
                    $controlled = $true
                }
                finally { Restore-MacCleanupRight }
                if (-not $controlled -or [IO.File]::Exists((Join-Path $state 'mac-authorization.pending'))) { throw 'Authorization failure-restoration control did not complete.' }
                Write-CiLog INFO 'Controlled cleanup failure restored original authorization policy.'
            }
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
                # The Rust driver sandboxes every copied CLI child; native trust ownership stays outside.
                $null = Invoke-Bounded $runner $arguments 130 -Isolated
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
                $store = [Security.Cryptography.X509Certificates.X509Store]::new('Root','LocalMachine')
                try {
                    Write-CiLog INFO 'Trust stage=windows-machine-root-open'
                    $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
                    if (@($store.Certificates | Where-Object Thumbprint -CEQ $fingerprint).Count) { throw 'Preexisting root collision refused.' }
                    $ledger = Read-Ledger
                    $ledger.roots += @{ fingerprint = $fingerprint; path = $owned }
                    Save-Ledger $ledger
                    Write-CiLog INFO 'Trust stage=windows-machine-root-import'
                    $store.Add($cert)
                    Write-CiLog INFO 'Trust stage=windows-machine-root-verify'
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
