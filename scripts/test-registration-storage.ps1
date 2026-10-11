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
    Write-RegistrationLog INFO 'Registration storage regression suite completed.'
} finally {
    Set-Item Function:\Publish-RegistrationFile $originalPublish
    $env:PATH = $oldPath
    $env:CLAUDE_CONFIG_DIR = $oldClaudeConfigDir
    if (Test-Path -LiteralPath $fixture) { Remove-Item -LiteralPath $fixture -Recurse -Force }
}
