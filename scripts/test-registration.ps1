$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$fixture = Join-Path $root ('.tmp/registration-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($fixture) | Out-Null
$desktop = Join-Path $fixture 'desktop.json'
$code = Join-Path $fixture 'code.json'
$legacy = @{ command = 'node'; args = @((Join-Path $root 'src/bridge.mjs')) }
$unrelated = @{ command = 'untouched'; args = @('fixture'); env = @{ PRIVATE_FIXTURE = 'not-a-real-secret' } }
$original = @{ mcpServers = @{ agensi = $legacy; other = $unrelated }; keep = @{ theme = 'fixture' } }
$before = $original | ConvertTo-Json -Depth 20 -Compress
try {
    foreach ($path in @($desktop, $code)) { [IO.File]::WriteAllText($path, $before, [Text.UTF8Encoding]::new($false)) }
    & pwsh -NoProfile -File (Join-Path $root 'scripts/register.ps1') -UpgradeProjectEntry -DesktopConfigPath $desktop -CodeConfigPath $code
    if ($LASTEXITCODE -ne 0) { throw 'Fixture registration failed.' }
    foreach ($path in @($desktop, $code)) {
        $saved = [IO.File]::ReadAllText($path) | ConvertFrom-Json -AsHashtable
        if (($saved.mcpServers.other | ConvertTo-Json -Depth 10 -Compress) -cne ($unrelated | ConvertTo-Json -Depth 10 -Compress)) { throw 'Unrelated entry changed.' }
        if ($saved.keep.theme -ne 'fixture') { throw 'Unrelated settings changed.' }
        if ($saved.mcpServers.agensi.command -ne (Join-Path $root 'bin/agensi-mcp.exe') -or $saved.mcpServers.agensi.env.TEMP -ne (Join-Path $root '.tmp')) { throw 'Native/project-local entry missing.' }
    }
    $blocked = @{ mcpServers = @{ agensi = @{ command = 'unrelated-server'; args = @('other') } } } | ConvertTo-Json -Depth 10
    [IO.File]::WriteAllText($desktop, $blocked, [Text.UTF8Encoding]::new($false))
    & pwsh -NoProfile -File (Join-Path $root 'scripts/register.ps1') -UpgradeProjectEntry -DesktopConfigPath $desktop -CodeConfigPath $code 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0 -or [IO.File]::ReadAllText($desktop) -cne $blocked) { throw 'Unrelated registration was overwritten.' }
    Write-Output 'Registration upgrade, unrelated-data preservation and refusal checks passed.'
} finally {
    if (Test-Path -LiteralPath $fixture) { Remove-Item -LiteralPath $fixture -Recurse -Force }
}
