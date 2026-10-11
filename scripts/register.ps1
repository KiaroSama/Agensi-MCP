#requires -Version 7.0
param([switch]$DryRun, [switch]$UpgradeProjectEntry, [string]$DesktopConfigPath, [string]$CodeConfigPath)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'registration-storage.ps1')
$root = Split-Path $PSScriptRoot -Parent
$binary = Join-Path $root 'bin/agensi-mcp.exe'
if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'Download/extract the native executable before registration.' }
$tempRoot = Join-Path $root '.tmp'
$entry = @{ command = $binary; args = @('bridge'); env = @{ TEMP = $tempRoot; TMP = $tempRoot; TMPDIR = $tempRoot } }
function Test-ProjectEntry($Existing) {
    if (-not ($Existing -is [System.Collections.IDictionary])) { return $false }
    if ($Existing.command -eq $binary -and (@($Existing.args) -join "`0") -eq 'bridge') { return $true }
    $legacy = Join-Path $root 'src/bridge.mjs'
    return @($Existing.args).Count -eq 1 -and $Existing.args[0] -eq $legacy -and [IO.Path]::GetFileNameWithoutExtension([string]$Existing.command) -eq 'node'
}
function Test-ExactEntry($Existing) {
    return ($Existing -is [System.Collections.IDictionary]) -and $Existing.command -eq $entry.command -and
        (@($Existing.args) -join "`0") -eq 'bridge' -and ($Existing.env -is [System.Collections.IDictionary]) -and
        $Existing.env.TEMP -eq $tempRoot -and $Existing.env.TMP -eq $tempRoot -and $Existing.env.TMPDIR -eq $tempRoot
}
try {
    if (-not $DesktopConfigPath) {
        $normal = Join-Path $env:APPDATA 'Claude/claude_desktop_config.json'
        $packages = Join-Path $env:LOCALAPPDATA 'Packages'
        $candidates = @()
        if (Test-Path -LiteralPath $packages) {
            $candidates = @(Get-ChildItem -LiteralPath $packages -Directory -Filter 'Claude_*' | ForEach-Object {
                Join-Path $_.FullName 'LocalCache/Roaming/Claude/claude_desktop_config.json'
            } | Where-Object { Test-Path -LiteralPath $_ })
        }
        if ($candidates.Count -gt 1) { throw 'Multiple Desktop installations found. Supply -DesktopConfigPath.' }
        $DesktopConfigPath = if ($candidates.Count -eq 1) { $candidates[0] } else { $normal }
    }
    if (-not $CodeConfigPath) {
        $codeHome = if ($env:CLAUDE_CONFIG_DIR) { $env:CLAUDE_CONFIG_DIR } else { $HOME }
        $CodeConfigPath = Join-Path $codeHome '.claude.json'
    }
    $desktop = Get-RegistrationSnapshot $DesktopConfigPath
    $code = Get-RegistrationSnapshot $CodeConfigPath
    if ($desktop.Path -ieq $code.Path) { throw 'Desktop and Code require distinct configuration targets.' }
    $changes = @()
    foreach ($snapshot in @($desktop, $code)) {
        $config = $snapshot.Value
        if (-not $config.Contains('mcpServers')) { $config.mcpServers = @{} }
        if (-not ($config.mcpServers -is [System.Collections.IDictionary])) { throw 'MCP settings must be a JSON object.' }
        if ($config.mcpServers.Contains('agensi')) {
            $existing = $config.mcpServers.agensi
            if (Test-ExactEntry $existing) { continue }
            if (-not ($UpgradeProjectEntry -and (Test-ProjectEntry $existing))) { throw 'A different Agensi registration exists. Review it before replacement.' }
            # Preserve extra client metadata and environment keys for this owned entry.
            $replacement = $existing
            if ($replacement.env -and -not ($replacement.env -is [System.Collections.IDictionary])) { throw 'Existing registration environment is not an object.' }
            if (-not $replacement.env) { $replacement.env = @{} }
            $replacement.command = $binary
            $replacement.args = @('bridge')
            foreach ($name in @('TEMP','TMP','TMPDIR')) { $replacement.env[$name] = $tempRoot }
        } else { $replacement = $entry }
        $config.mcpServers.agensi = $replacement
        $snapshot.NewBytes = [Text.UTF8Encoding]::new($false).GetBytes(($config | ConvertTo-Json -Depth 100))
        $changes += $snapshot
    }
    Write-RegistrationLog DEBUG ('Validated ' + $changes.Count + ' registration change(s); config contents omitted.')
    if ($DryRun) {
        Write-Output ('Desktop target: ' + $desktop.Path)
        Write-Output ('Code target: ' + $code.Path)
        Write-Output 'Both targets validated. Dry run: no files or registrations changed.'
        return
    }
    # Explicit target paths are honored directly; never invoke a CLI with an implicit HOME.
    [IO.Directory]::CreateDirectory($tempRoot) | Out-Null
    Invoke-RegistrationTransaction @($desktop, $code)
    Write-Output 'Agensi registered in the validated Desktop and Code targets. Restart clients after selecting your route.'
} catch {
    Write-RegistrationLog ERROR $_.Exception.Message
    throw
}
