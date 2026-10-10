param([switch]$DryRun, [switch]$UpgradeProjectEntry, [string]$DesktopConfigPath, [string]$CodeConfigPath)
$ErrorActionPreference = 'Stop'
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
$old = if (Test-Path $DesktopConfigPath) { [IO.File]::ReadAllText($DesktopConfigPath, [Text.Encoding]::UTF8) } else { $null }
$config = if ($old) { $old | ConvertFrom-Json -AsHashtable } else { @{} }
if (-not ($config -is [System.Collections.IDictionary])) { throw 'Desktop config is not a JSON object.' }
if (-not $config.ContainsKey('mcpServers')) { $config.mcpServers = @{} }
if (-not ($config.mcpServers -is [System.Collections.IDictionary])) { throw 'Desktop MCP config is not an object.' }
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
$config.mcpServers.agensi = $entry
if ($DryRun) {
    Write-Output ('Desktop target: ' + $DesktopConfigPath)
    Write-Output 'Code target: user scope; name: agensi; transport: local stdio bridge to Streamable HTTP'
    Write-Output ($entry | ConvertTo-Json -Depth 5)
    exit 0
}
[IO.Directory]::CreateDirectory($tempRoot) | Out-Null
# Check for an existing user entry without emitting unrelated credentials.
$codePath = if ($CodeConfigPath) { $CodeConfigPath } else { Join-Path $HOME '.claude.json' }
if (Test-Path $codePath) {
    $codeOriginal = [IO.File]::ReadAllText($codePath, [Text.Encoding]::UTF8)
    $code = $codeOriginal | ConvertFrom-Json -AsHashtable
    if ($code.mcpServers -and $code.mcpServers.ContainsKey('agensi')) {
        $existing = $code.mcpServers.agensi
        if (-not (Test-ExactEntry $existing)) {
            if (-not ($UpgradeProjectEntry -and (Test-ProjectEntry $existing))) { throw 'A different Agensi Code registration exists. Review it before replacement.' }
            $code.mcpServers.agensi = $entry
            $codeNow = [IO.File]::ReadAllText($codePath, [Text.Encoding]::UTF8)
            if (($codeNow | ConvertFrom-Json -AsHashtable | ConvertTo-Json -Depth 100 -Compress) -cne ($codeOriginal | ConvertFrom-Json -AsHashtable | ConvertTo-Json -Depth 100 -Compress)) { throw 'Code settings changed concurrently; retry registration.' }
            $codeTemp = $codePath + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
            try {
                [IO.File]::WriteAllText($codeTemp, ($code | ConvertTo-Json -Depth 100), [Text.UTF8Encoding]::new($false))
                [IO.File]::Move($codeTemp, $codePath, $true)
            } finally { if (Test-Path -LiteralPath $codeTemp) { Remove-Item -LiteralPath $codeTemp } }
        }
    } else {
        $claude = Get-Command claude -ErrorAction Stop
        & $claude.Source mcp add-json --scope user agensi ($entry | ConvertTo-Json -Compress -Depth 5)
        if ($LASTEXITCODE -ne 0) { throw 'Code registration failed; Desktop unchanged.' }
    }
} else {
    $claude = Get-Command claude -ErrorAction Stop
    & $claude.Source mcp add-json --scope user agensi ($entry | ConvertTo-Json -Compress -Depth 5)
    if ($LASTEXITCODE -ne 0) { throw 'Code registration failed; Desktop unchanged.' }
}
$now = if (Test-Path $DesktopConfigPath) { [IO.File]::ReadAllText($DesktopConfigPath, [Text.Encoding]::UTF8) } else { $null }
if ($now -cne $old) { throw 'Desktop settings changed concurrently; retry registration.' }
[IO.Directory]::CreateDirectory((Split-Path $DesktopConfigPath -Parent)) | Out-Null
$temp = $DesktopConfigPath + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
try {
    [IO.File]::WriteAllText($temp, ($config | ConvertTo-Json -Depth 100), [Text.UTF8Encoding]::new($false))
    [IO.File]::Move($temp, $DesktopConfigPath, $true)
} finally { if (Test-Path $temp) { Remove-Item -LiteralPath $temp } }
Write-Output 'Agensi registered in Desktop and global Code. Restart the clients after selecting your route.'
