param([switch]$Debug)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'Rust 1.99+ toolchain is required; no automatic global install.' }
$names = @('CARGO_HOME', 'CARGO_TARGET_DIR', 'CARGO_BUILD_JOBS', 'TEMP', 'TMP', 'TMPDIR', 'AGENSI_SOURCE_COMMIT')
$previous = @{}
foreach ($name in $names) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
Push-Location $root
try {
    $env:CARGO_HOME = Join-Path $root '.cache/cargo'
    $env:CARGO_TARGET_DIR = Join-Path $root 'target'
    $env:CARGO_BUILD_JOBS = '4'
    $tempRoot = Join-Path $root '.tmp'
    foreach ($path in @($env:CARGO_HOME, $env:CARGO_TARGET_DIR, $tempRoot)) { [IO.Directory]::CreateDirectory($path) | Out-Null }
    foreach ($name in @('TEMP', 'TMP', 'TMPDIR')) { [Environment]::SetEnvironmentVariable($name, $tempRoot, 'Process') }
    if (-not $Debug) {
        $env:AGENSI_SOURCE_COMMIT = (& git -C $root rev-parse HEAD).Trim()
        if ($LASTEXITCODE -ne 0 -or $env:AGENSI_SOURCE_COMMIT -cnotmatch '^[0-9a-f]{40}$') { throw 'Verified source commit is required for a completed build.' }
        & (Join-Path $PSScriptRoot 'sign-release.ps1') -Path (Join-Path $root 'target/release/agensi-mcp.exe') -Preflight
    }
    $arguments = @('build', '--locked')
    $profile = if ($Debug) { 'debug' } else { 'release' }
    if (-not $Debug) { $arguments += '--release' }
    & cargo @arguments
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    if ($Debug) { Write-Output 'Internal debug intermediate built under target/debug; installed binary unchanged.'; return }
    $candidate = Join-Path $root "target/$profile/agensi-mcp.exe"
    if ([IO.File]::Exists($candidate + '.sig')) { [IO.File]::Delete($candidate + '.sig') }
    $version = [regex]::Match([IO.File]::ReadAllText((Join-Path $root 'Cargo.toml'), [Text.UTF8Encoding]::new($false,$true)), '(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"').Groups[1].Value
    & (Join-Path $PSScriptRoot 'sign-release.ps1') -Path $candidate -Platform windows-x64 -SourceCommit $env:AGENSI_SOURCE_COMMIT -Version $version
    $destination = Join-Path $root 'bin'
    [IO.Directory]::CreateDirectory($destination) | Out-Null
    $staged = Join-Path $destination ('verified-' + [guid]::NewGuid().ToString('N') + '.exe')
    $installed = Join-Path $destination 'agensi-mcp.exe'
    $backup = Join-Path $destination ('previous-' + [guid]::NewGuid().ToString('N') + '.exe')
    $hadBinary = [IO.File]::Exists($installed)
    $hadSignature = [IO.File]::Exists($installed + '.sig')
    $replaced = $false
    try {
        Copy-Item -LiteralPath $candidate -Destination $staged
        Copy-Item -LiteralPath ($candidate + '.sig') -Destination ($staged + '.sig')
        & (Join-Path $PSScriptRoot 'verify-release.ps1') -Path $staged -Platform windows-x64 -SourceCommit $env:AGENSI_SOURCE_COMMIT -Version $version
        if ($hadBinary) { [IO.File]::Copy($installed, $backup, $false) }
        if ($hadSignature) { [IO.File]::Copy(($installed + '.sig'), ($backup + '.sig'), $false) }
        [IO.File]::Move($staged, $installed, $true)
        $replaced = $true
        [IO.File]::Move(($staged + '.sig'), ($installed + '.sig'), $true)
        & (Join-Path $PSScriptRoot 'verify-release.ps1') -Path $installed -Platform windows-x64 -SourceCommit $env:AGENSI_SOURCE_COMMIT -Version $version
        if ($hadBinary -or $hadSignature) { Write-Output 'Previous executable/signature retained under bin/previous-* for verified rollback; recycle after successful activation.' }
    } catch {
        if ($replaced) {
            if ($hadBinary) { [IO.File]::Copy($backup, $installed, $true) } elseif ([IO.File]::Exists($installed)) { [IO.File]::Delete($installed) }
            if ($hadSignature) { [IO.File]::Copy(($backup + '.sig'), ($installed + '.sig'), $true) } elseif ([IO.File]::Exists($installed + '.sig')) { [IO.File]::Delete($installed + '.sig') }
        }
        throw
    } finally { foreach ($path in @($staged, ($staged + '.sig'))) { if ([IO.File]::Exists($path)) { [IO.File]::Delete($path) } } }
    Write-Output 'Private-signed binary verified before installation in bin/agensi-mcp.exe.'
} finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
    Pop-Location
}
