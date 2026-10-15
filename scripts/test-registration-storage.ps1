#requires -Version 7.0
$ErrorActionPreference = 'Stop'
$sourceRoot = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'registration-storage.ps1')
$fixture = Join-Path $sourceRoot ('.tmp/registration-audit-' + [guid]::NewGuid().ToString('N'))
$root = Join-Path $fixture 'installation'
foreach ($directory in @($fixture, (Join-Path $root 'scripts'), (Join-Path $root 'bin'))) { [IO.Directory]::CreateDirectory($directory) | Out-Null }
foreach ($name in @('register.ps1', 'registration-storage.ps1')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $root "scripts/$name") }
# Registration validates existence only; no executable is launched by these fixtures.
[IO.File]::WriteAllBytes((Join-Path $root 'bin/agensi-mcp.exe'), [byte[]]::new(0))
$utf8 = [Text.UTF8Encoding]::new($false)
$desktop = Join-Path $fixture 'desktop.json'
$code = Join-Path $fixture 'explicit-code.json'
$oldPath = $env:PATH
$oldClaudeConfigDir = $env:CLAUDE_CONFIG_DIR
$shim = Join-Path $fixture 'cli-shim'
[IO.Directory]::CreateDirectory($shim) | Out-Null
$marker = Join-Path $fixture 'unexpected-cli.txt'
[IO.File]::WriteAllText((Join-Path $shim 'claude.cmd'), "@echo off`r`necho invoked> `"$marker`"`r`nexit /b 93`r`n", $utf8)
$env:PATH = $shim + [IO.Path]::PathSeparator + $env:PATH
function Assert-Case([bool]$Condition, [string]$Name) {
    if (-not $Condition) { Write-RegistrationLog ERROR ('FAIL ' + $Name); throw 'Registration regression failed.' }
    Write-RegistrationLog INFO ('PASS ' + $Name)
}
function Invoke-IsolatedRegistration([string[]]$Options = @()) {
    & pwsh -NoProfile -File (Join-Path $root 'scripts/register.ps1') @Options -DesktopConfigPath $desktop -CodeConfigPath $code 2>&1 | Out-Null
    return $LASTEXITCODE
}
function Invoke-RootValidation([string[]]$Options = @()) {
    try {
        & (Join-Path $root 'scripts/register.ps1') @Options -DesktopConfigPath $desktop -CodeConfigPath $code 2>&1 | Out-Null
        return 0
    } catch { return 1 }
}
function Set-Fixture([string]$Path, [string]$Text) { [IO.File]::WriteAllText($Path, $Text, $utf8) }
$originalPublish = (Get-Command Publish-RegistrationFile).ScriptBlock
try {
    $unrelated = '{"keep":{"theme":"fixture"},"mcpServers":{"other":{"command":"untouched"}}}'
    Set-Fixture $desktop $unrelated
    Assert-Case ((Invoke-IsolatedRegistration) -eq 0) 'missing explicit Code target is created without invoking the global CLI'
    Assert-Case ([IO.File]::Exists($code) -and -not [IO.File]::Exists($marker)) 'explicit path honored and CLI never called'
    if ($IsWindows) { Assert-Case ((Get-Acl -LiteralPath $code).AreAccessRulesProtected) 'new config uses a private protected ACL' }
    $saved = [IO.File]::ReadAllText($desktop) | ConvertFrom-Json -AsHashtable
    Assert-Case ($saved.keep.theme -eq 'fixture' -and $saved.mcpServers.other.command -eq 'untouched') 'unrelated client settings preserved'
    $beforeDesktop = Read-RegistrationBytes $desktop
    $beforeCode = Read-RegistrationBytes $code
    Assert-Case ((Invoke-IsolatedRegistration) -eq 0) 'idempotent registration succeeds'
    Assert-Case ((Test-RegistrationBytesEqual $beforeDesktop (Read-RegistrationBytes $desktop)) -and (Test-RegistrationBytesEqual $beforeCode (Read-RegistrationBytes $code))) 'idempotent registration preserves exact file bytes'
    $profile = Join-Path $fixture 'isolated-profile'
    [IO.Directory]::CreateDirectory($profile) | Out-Null
    $env:CLAUDE_CONFIG_DIR = $profile
    & pwsh -NoProfile -File (Join-Path $root 'scripts/register.ps1') -DesktopConfigPath $desktop 2>&1 | Out-Null
    Assert-Case ($LASTEXITCODE -eq 0 -and [IO.File]::Exists((Join-Path $profile '.claude.json'))) 'default Code target follows the selected CLAUDE_CONFIG_DIR profile'
    Assert-Case (-not [IO.File]::Exists($marker)) 'profile registration does not invoke the implicit global CLI'
    $env:CLAUDE_CONFIG_DIR = $oldClaudeConfigDir
    Set-Fixture $code '{"mcpServers":{"agensi":{"command":"foreign-server","args":[]}}}'
    $foreign = Read-RegistrationBytes $code
    Assert-Case ((Invoke-IsolatedRegistration @('-DryRun')) -ne 0) 'dry run rejects a foreign Code entry'
    Assert-Case ((Test-RegistrationBytesEqual $beforeDesktop (Read-RegistrationBytes $desktop)) -and (Test-RegistrationBytesEqual $foreign (Read-RegistrationBytes $code))) 'failed validation changes neither target'
    $timestampValues = @('2026-10-15T12:34:56Z', '2026-10-15T12:34:56.1234567+05:30', '2026-10-15T12:34:56.7654321-07:00', 'ordinary café text')
    foreach ($upgrade in @($false, $true)) {
        $servers = @{ other = @{ env = @{ DATE = $timestampValues[1] } } }
        if ($upgrade) { $servers.agensi = @{ command = 'node'; args = @((Join-Path $root 'src/bridge.mjs')) } }
        $text = '{"keep":{"dates":' + (ConvertTo-Json -InputObject $timestampValues -Compress) + ',"nested":[{"":"empty-key","Case":"upper","case":"lower","nullable":null,"$type":"unknown-fixture-type","$id":"literal-id"},[],[null,7,true],["single"],9223372036854775807,1.25]},"mcpServers":' + ($servers | ConvertTo-Json -Depth 10 -Compress) + '}'
        foreach ($path in @($desktop, $code)) { Set-Fixture $path $text }
        $options = if ($upgrade) { @('-UpgradeProjectEntry') } else { @() }
        Assert-Case ((Invoke-IsolatedRegistration $options) -eq 0) 'new and upgraded registrations serialize unrelated JSON'
        foreach ($path in @($desktop, $code)) {
            $document = [System.Text.Json.JsonDocument]::Parse([IO.File]::ReadAllText($path, $utf8))
            try {
                $dates = $document.RootElement.GetProperty('keep').GetProperty('dates')
                for ($index = 0; $index -lt $timestampValues.Count; $index++) {
                    Assert-Case ($dates[$index].ValueKind -eq [System.Text.Json.JsonValueKind]::String -and $dates[$index].GetString() -ceq $timestampValues[$index]) 'timestamp and Unicode strings retain exact decoded contents'
                }
                $nested = $document.RootElement.GetProperty('keep').GetProperty('nested')
                Assert-Case ($nested[0].GetProperty('').GetString() -ceq 'empty-key' -and $nested[0].GetProperty('Case').GetString() -ceq 'upper' -and $nested[0].GetProperty('case').GetString() -ceq 'lower') 'empty and case-distinct keys survive serialization'
                Assert-Case ($nested[0].GetProperty('nullable').ValueKind -eq [System.Text.Json.JsonValueKind]::Null -and $nested[1].GetArrayLength() -eq 0 -and $nested[2][0].ValueKind -eq [System.Text.Json.JsonValueKind]::Null -and $nested[2][1].GetInt32() -eq 7 -and $nested[2][2].GetBoolean()) 'nested arrays nulls numbers and booleans retain their types'
                Assert-Case ($nested[0].GetProperty('$type').GetString() -ceq 'unknown-fixture-type' -and $nested[0].GetProperty('$id').GetString() -ceq 'literal-id') 'JSON metadata stays ordinary unrelated data'
                Assert-Case ($nested[3].GetArrayLength() -eq 1 -and $nested[3][0].GetString() -ceq 'single' -and $nested[4].GetInt64() -eq [long]::MaxValue -and $nested[5].GetDouble() -eq 1.25) 'single arrays large integers and fractional numbers retain values'
            } finally { $document.Dispose() }
        }
    }
    foreach ($invalid in @('[]', '[{}]', '[{"mcpServers":{}}]', '[{},{}]', 'null', '"text"', '7', 'true', '{"mcpServers":[]}', '{"mcpServers":null}', '{broken')) {
        foreach ($target in @($desktop, $code)) {
            Set-Fixture $desktop '{}'
            Set-Fixture $code '{}'
            Set-Fixture $target $invalid
            $unchangedDesktop = Read-RegistrationBytes $desktop
            $unchangedCode = Read-RegistrationBytes $code
            foreach ($dryRun in @($false, $true)) {
                $options = if ($dryRun) { @('-DryRun') } else { @() }
                Assert-Case ((Invoke-RootValidation $options) -ne 0) 'normal and dry-run reject invalid roots at either target'
                Assert-Case ((Test-RegistrationBytesEqual $unchangedDesktop (Read-RegistrationBytes $desktop)) -and (Test-RegistrationBytesEqual $unchangedCode (Read-RegistrationBytes $code))) 'invalid roots preserve both targets byte-for-byte'
            }
        }
    }
    foreach ($fallback in @($false, $true)) {
        Set-Fixture $desktop '{}'
        Set-Fixture $code '{}'
        $beforeDesktop = Read-RegistrationBytes $desktop
        $beforeCode = Read-RegistrationBytes $code
        $logDirectory = Join-Path $root 'logs'
        if ($fallback) {
            Remove-Item -LiteralPath $logDirectory -Recurse -Force
            Set-Fixture $logDirectory 'Controlled logging-init failure.'
        }
        $count = if ($fallback) { 0 } else { @(Get-ChildItem -LiteralPath $logDirectory -File).Count }
        $output = & (Join-Path $root 'scripts/register.ps1') -DryRun -DesktopConfigPath $desktop -CodeConfigPath $code 2>&1 | Out-String
        Assert-Case ((Test-RegistrationBytesEqual $beforeDesktop (Read-RegistrationBytes $desktop)) -and (Test-RegistrationBytesEqual $beforeCode (Read-RegistrationBytes $code))) 'dry-run preserves both client configurations'
        Assert-Case ($output.Contains('client configuration files and registrations unchanged') -and -not $output.Contains('no files or registrations changed')) 'dry-run reports only suppressed client mutations'
        if ($fallback) {
            Assert-Case ($output.Contains('stderr only (file logging unavailable)') -and [IO.File]::Exists($logDirectory)) 'dry-run accurately reports logging fallback without claiming a file'
            Remove-Item -LiteralPath $logDirectory -Force
        } else {
            Assert-Case ($output.Contains('file log and stderr') -and @(Get-ChildItem -LiteralPath $logDirectory -File).Count -eq ($count + 1)) 'dry-run reports its actual new diagnostic file'
        }
    }
    foreach ($concurrent in @($false, $true)) {
        Set-Fixture $desktop '{"original":1}'
        Set-Fixture $code '{"original":2}'
        $first = Get-RegistrationSnapshot $desktop
        $second = Get-RegistrationSnapshot $code
        $first.NewBytes = $utf8.GetBytes('{"updated":1}')
        $second.NewBytes = $utf8.GetBytes('{"updated":2}')
        $script:publishCount = 0
        $script:mutateConcurrent = $concurrent
        function Publish-RegistrationFile([string]$Stage, [string]$Path) {
            $script:publishCount++
            if ($script:publishCount -eq 2) {
                if ($script:mutateConcurrent) { Set-Fixture $desktop '{"concurrent":true}' }
                throw 'Injected second-publication failure.'
            }
            & $originalPublish $Stage $Path
        }
        $failed = $false
        try { Invoke-RegistrationTransaction @($first, $second) } catch { $failed = $true }
        Assert-Case $failed 'a partial publication never claims success'
        Assert-Case (Test-RegistrationBytesEqual $second.Bytes (Read-RegistrationBytes $code)) 'second target remains untouched after injected failure'
        if ($concurrent) {
            Assert-Case ([IO.File]::ReadAllText($desktop) -ceq '{"concurrent":true}') 'rollback never overwrites a noncooperating concurrent edit'
            Assert-Case ([IO.File]::Exists($first.Backup)) 'private recovery bytes retained when safe rollback is impossible'
        } else {
            Assert-Case (Test-RegistrationBytesEqual $first.Bytes (Read-RegistrationBytes $desktop)) 'first target rolled back byte-for-byte'
            Assert-Case (-not [IO.File]::Exists($first.Backup)) 'successful rollback leaves no recovery copy'
        }
        Set-Item Function:\Publish-RegistrationFile $originalPublish
    }
    foreach ($invalid in @('', ' ', "`n", 'null')) {
        Set-Fixture $code $invalid
        $before = Read-RegistrationBytes $code
        $failed = $false
        try { Get-RegistrationSnapshot $code | Out-Null } catch { $failed = $true }
        Assert-Case $failed 'empty or null existing config is refused'
        Assert-Case (Test-RegistrationBytesEqual $before (Read-RegistrationBytes $code)) 'invalid existing bytes preserved'
    }
    $limitPath = Join-Path $fixture 'limit.json'
    foreach ($size in @((32MB - 1), 32MB, (32MB + 1))) {
        $stream = [IO.File]::Create($limitPath)
        try { $stream.SetLength($size) } finally { $stream.Dispose() }
        $failed = $false
        try { $bytes = Read-RegistrationBytes $limitPath } catch { $failed = $true }
        Assert-Case ($failed -eq ($size -gt 32MB)) 'client config read limit is exact'
        if (-not $failed) { Assert-Case ($bytes.Length -eq $size) 'bounded reader retains exact byte count' }
    }
    Set-Fixture $desktop '{"original":1}'
    Set-Fixture $code '{"original":2}'
    $first = Get-RegistrationSnapshot $desktop
    $second = Get-RegistrationSnapshot $code
    $first.NewBytes = $utf8.GetBytes('{"updated":1}')
    $second.NewBytes = $utf8.GetBytes('{"updated":2}')
    $script:publishCount = 0
    function Publish-RegistrationFile([string]$Stage, [string]$Path) {
        $script:publishCount++
        if ($script:publishCount -eq 2) {
            Set-Fixture $code '{"external-after-failure":true}'
            throw 'Injected replacement changed target before reporting failure.'
        }
        & $originalPublish $Stage $Path
    }
    $failed = $false
    try { Invoke-RegistrationTransaction @($first, $second) } catch { $failed = $true }
    Assert-Case $failed 'replacement exception after mutation is not success'
    Assert-Case ([IO.File]::ReadAllText($code, $utf8) -ceq '{"external-after-failure":true}') 'unknown partial replacement is not overwritten'
    Assert-Case ([IO.File]::Exists($second.Backup)) 'unconfirmed replacement retains original recovery bytes'
    Assert-Case (Test-RegistrationBytesEqual $first.Bytes (Read-RegistrationBytes $desktop)) 'confirmed prior publication is restored'
    Set-Item Function:\Publish-RegistrationFile $originalPublish
    $oversized = Get-RegistrationSnapshot $desktop
    $oversized.NewBytes = [byte[]]::new(32MB + 1)
    $failed = $false
    try { Invoke-RegistrationTransaction @($oversized) } catch { $failed = $true }
    Assert-Case ($failed -and (Test-RegistrationBytesEqual $oversized.Bytes (Read-RegistrationBytes $desktop))) 'oversized output is refused before publishing'
    Initialize-RegistrationLog $fixture
    foreach ($level in @('INFO','WARNING','ERROR','DEBUG')) { Write-RegistrationLog $level 'Safe UTF-8 fixture café.' }
    Close-RegistrationLog
    Initialize-RegistrationLog $fixture
    Write-RegistrationLog INFO 'Second invocation fixture.'
    Close-RegistrationLog
    $logs = @(Get-ChildItem -LiteralPath (Join-Path $fixture 'logs') -File)
    Assert-Case ($logs.Count -eq 2) 'file logging uses a unique file per run'
    $text = [IO.File]::ReadAllText($logs[0].FullName, [Text.UTF8Encoding]::new($false, $true)) + [IO.File]::ReadAllText($logs[1].FullName, [Text.UTF8Encoding]::new($false, $true))
    Assert-Case ($text.Contains('café.') -and $text.Contains('UTC] [DEBUG] [registration]')) 'UTF-8 UTC levels roundtrip and handlers close'
    Write-RegistrationLog INFO 'Registration storage regression suite completed.'
} finally {
    Set-Item Function:\Publish-RegistrationFile $originalPublish
    $env:PATH = $oldPath
    $env:CLAUDE_CONFIG_DIR = $oldClaudeConfigDir
    if (Test-Path -LiteralPath $fixture) { Remove-Item -LiteralPath $fixture -Recurse -Force }
}
