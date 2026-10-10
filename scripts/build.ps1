param([switch]$Debug)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'Rust 1.99+ toolchain is required; no automatic global install.' }
$env:CARGO_HOME = Join-Path $root '.cache/cargo'
$env:CARGO_TARGET_DIR = Join-Path $root 'target'
$env:CARGO_BUILD_JOBS = '4'
Push-Location $root
try {
    $arguments = @('build', '--locked')
    $profile = if ($Debug) { 'debug' } else { 'release' }
    if (-not $Debug) { $arguments += '--release' }
    & cargo @arguments
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $destination = Join-Path $root 'bin'
    [IO.Directory]::CreateDirectory($destination) | Out-Null
    Copy-Item (Join-Path $root "target/$profile/agensi-mcp.exe") (Join-Path $destination 'agensi-mcp.exe')
    Write-Output 'Native binary built in bin/agensi-mcp.exe.'
} finally { Pop-Location }
