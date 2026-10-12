#requires -Version 7.0
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'registration-storage.ps1')
$fixture = Join-Path $root ('.tmp/registration-audit-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($fixture) | Out-Null
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
    foreach ($invalid in @('[]', '{"mcpServers":[]}', '{"mcpServers":null}', '{broken')) {
        Set-Fixture $code $invalid
        Assert-Case ((Invoke-IsolatedRegistration) -ne 0) 'invalid Code config shape is refused before writing Desktop'
        Assert-Case (Test-RegistrationBytesEqual $beforeDesktop (Read-RegistrationBytes $desktop)) 'invalid Code config preserves Desktop bytes'
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
