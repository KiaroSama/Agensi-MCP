param([switch]$DryRun, [switch]$UpgradeProjectEntry, [string]$DesktopConfigPath, [string]$CodeConfigPath)
$ErrorActionPreference = 'Stop'
function Write-RegistrationLog {
    param([ValidateSet('INFO', 'WARNING', 'ERROR', 'DEBUG')][string]$Level, [string]$Message)
    [Console]::Error.WriteLine(('[{0:yyyy-MM-dd HH:mm:ss} UTC] [{1}] [registration] {2}' -f [DateTime]::UtcNow, $Level, $Message))
}
Write-RegistrationLog INFO 'Validating registration targets; configuration values omitted.'
function Read-ClientConfig([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return @{ Original = $null; Config = @{} } }
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or $item.Length -gt 1MB) { throw 'Client configuration is not a bounded regular file.' }
    $original = [IO.File]::ReadAllText($Path, [Text.Encoding]::UTF8)
    if ([string]::IsNullOrWhiteSpace($original)) { throw 'Existing client configuration is empty; preserving it.' }
    try { $value = $original | ConvertFrom-Json -AsHashtable } catch { throw 'Client configuration is invalid JSON; values omitted.' }
    if (-not ($value -is [System.Collections.IDictionary])) { throw 'Client configuration is not a JSON object.' }
    if ($value.ContainsKey('mcpServers') -and -not ($value.mcpServers -is [System.Collections.IDictionary])) { throw 'Client MCP configuration is not an object.' }
    return @{ Original = $original; Config = $value }
}
function Save-ClientConfig([string]$Path, $Original, $Config) {
    $now = if (Test-Path -LiteralPath $Path) { [IO.File]::ReadAllText($Path, [Text.Encoding]::UTF8) } else { $null }
    if ($now -cne $Original) { throw 'Client settings changed concurrently; retry registration.' }
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($Path))) | Out-Null
    $temp = $Path + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
    try {
        [IO.File]::WriteAllText($temp, ($Config | ConvertTo-Json -Depth 100), [Text.UTF8Encoding]::new($false))
        [IO.File]::Move($temp, $Path, $true)
    } finally { if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp } }
}
$root = Split-Path $PSScriptRoot -Parent
$binary = Join-Path $root 'bin/agensi-mcp.exe'
if (-not (Test-Path $binary)) { throw 'Download/extract the native executable before registration.' }
$tempRoot = Join-Path $root '.tmp'
$entry = @{ command = $binary; args = @('bridge'); env = @{ TEMP = $tempRoot; TMP = $tempRoot; TMPDIR = $tempRoot } }
if (-not $DesktopConfigPath) {
    $normal = Join-Path $env:APPDATA 'Claude/claude_desktop_config.json'
    $packages = Join-Path $env:LOCALAPPDATA 'Packages'
    $candidates = @()
    if (Test-Path $packages) {
        $candidates = @(Get-ChildItem $packages -Directory -Filter 'Claude_*' | ForEach-Object {
            Join-Path $_.FullName 'LocalCache/Roaming/Claude/claude_desktop_config.json'
        } | Where-Object { Test-Path $_ })
    }
    if ($candidates.Count -gt 1) { throw 'Multiple Desktop installations found. Supply -DesktopConfigPath.' }
    $DesktopConfigPath = if ($candidates.Count -eq 1) { $candidates[0] } else { $normal }
}
$desktopState = Read-ClientConfig $DesktopConfigPath
$old = $desktopState.Original
$config = $desktopState.Config
if (-not $config.ContainsKey('mcpServers')) { $config.mcpServers = @{} }
function Test-ProjectEntry($Existing) {
    if ($Existing.command -eq $binary -and ($Existing.args -join "`0") -eq 'bridge') { return $true }
    $legacy = Join-Path $root 'src/bridge.mjs'
    return @($Existing.args).Count -eq 1 -and $Existing.args[0] -eq $legacy -and [IO.Path]::GetFileNameWithoutExtension($Existing.command) -eq 'node'
}
function Test-ExactEntry($Existing) {
    return $Existing.command -eq $entry.command -and ($Existing.args -join "`0") -eq ($entry.args -join "`0") -and
        $Existing.env.TEMP -eq $tempRoot -and $Existing.env.TMP -eq $tempRoot -and $Existing.env.TMPDIR -eq $tempRoot
}
if ($config.mcpServers.ContainsKey('agensi')) {
    $existing = $config.mcpServers.agensi
    if (-not (Test-ExactEntry $existing) -and -not ($UpgradeProjectEntry -and (Test-ProjectEntry $existing))) {
        throw 'A different Agensi Desktop registration exists. Review it before replacement.'
    }
}
# Preserve exact entries, including caller-owned extra environment settings.
if (-not $config.mcpServers.ContainsKey('agensi') -or -not (Test-ExactEntry $config.mcpServers.agensi)) { $config.mcpServers.agensi = $entry }
$codePath = if ($CodeConfigPath) { $CodeConfigPath } else { Join-Path $HOME '.claude.json' }
if ([IO.Path]::GetFullPath($codePath) -eq [IO.Path]::GetFullPath($DesktopConfigPath)) { throw 'Desktop and Code targets must be distinct.' }
$codeState = Read-ClientConfig $codePath
$codeOriginal = $codeState.Original
$code = $codeState.Config
if (-not $code.ContainsKey('mcpServers')) { $code.mcpServers = @{} }
$hasCodeEntry = $code.mcpServers.ContainsKey('agensi')
if ($hasCodeEntry -and -not (Test-ExactEntry $code.mcpServers.agensi) -and
    -not ($UpgradeProjectEntry -and (Test-ProjectEntry $code.mcpServers.agensi))) { throw 'A different Agensi Code registration exists. Review it before replacement.' }
if ($DryRun) {
    Write-Output ('Desktop target: ' + $DesktopConfigPath)
    Write-Output ('Code target: ' + $codePath)
    Write-Output ($entry | ConvertTo-Json -Depth 5)
    Write-RegistrationLog INFO 'Both registration targets validated; dry-run made no changes.'
    exit 0
}
[IO.Directory]::CreateDirectory($tempRoot) | Out-Null
if ($CodeConfigPath -or $hasCodeEntry) {
    # Explicit test/custom paths never invoke the CLI against a different user configuration.
    if (-not $hasCodeEntry -or -not (Test-ExactEntry $code.mcpServers.agensi)) {
        $code.mcpServers.agensi = $entry
        Save-ClientConfig $codePath $codeOriginal $code
    }
} else {
    $claude = Get-Command claude -ErrorAction Stop
    & $claude.Source mcp add-json --scope user agensi ($entry | ConvertTo-Json -Compress -Depth 5)
    if ($LASTEXITCODE -ne 0) { throw 'Code registration failed; Desktop unchanged.' }
}
Save-ClientConfig $DesktopConfigPath $old $config
Write-RegistrationLog INFO 'Registration completed for both selected targets.'
Write-Output 'Agensi registered in the selected Desktop and Code targets. Restart the clients after selecting your route.'
