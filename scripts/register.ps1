param([switch]$DryRun, [string]$DesktopConfigPath)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$node = (Get-Command node -ErrorAction Stop).Source
$claude = Get-Command claude -ErrorAction Stop
$entry = @{ command = $node; args = @((Join-Path $root 'src/bridge.mjs')) }
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
if ($config.mcpServers.ContainsKey('agensi')) {
    $existing = $config.mcpServers.agensi
    if ($existing.command -ne $entry.command -or ($existing.args -join "`0") -ne ($entry.args -join "`0")) {
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
# Check for an existing user entry without emitting unrelated credentials.
$codePath = Join-Path $HOME '.claude.json'
if (Test-Path $codePath) {
    $code = [IO.File]::ReadAllText($codePath, [Text.Encoding]::UTF8) | ConvertFrom-Json -AsHashtable
    if ($code.mcpServers -and $code.mcpServers.ContainsKey('agensi')) {
        $existing = $code.mcpServers.agensi
        if ($existing.command -ne $entry.command -or ($existing.args -join "`0") -ne ($entry.args -join "`0")) {
            throw 'A different Agensi Code registration exists. Review it before replacement.'
        }
    } else {
        & $claude.Source mcp add-json --scope user agensi ($entry | ConvertTo-Json -Compress -Depth 5)
        if ($LASTEXITCODE -ne 0) { throw 'Code registration failed; Desktop unchanged.' }
    }
} else {
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
