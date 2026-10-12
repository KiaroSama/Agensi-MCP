$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
function Write-AuditLog {
    param([ValidateSet('INFO', 'WARNING', 'ERROR', 'DEBUG')][string]$Level, [string]$Message)
    [Console]::Error.WriteLine(('[{0:yyyy-MM-dd HH:mm:ss} UTC] [{1}] [dependency-audit] {2}' -f [DateTime]::UtcNow, $Level, $Message))
}
$root = Split-Path $PSScriptRoot -Parent
$tools = Join-Path $root '.cache/audit-tools'
$database = Join-Path $root '.cache/advisory-db'
Push-Location $root
try {
    Write-AuditLog INFO 'Installing the pinned RustSec auditor from its locked dependency graph.'
    & cargo install cargo-audit --version 0.22.2 --locked --root $tools
    if ($LASTEXITCODE -ne 0) { throw 'Pinned auditor installation failed; no scan claimed.' }
    $executable = Join-Path $tools ('bin/cargo-audit' + $(if ($IsWindows) { '.exe' } else { '' }))
    Write-AuditLog DEBUG 'Auditing the complete committed lockfile with a fresh project-local advisory database.'
    & $executable audit --file (Join-Path $root 'Cargo.lock') --db $database --deny unsound
    if ($LASTEXITCODE -ne 0) { throw 'Dependency audit failed; review the advisory output before integration.' }
    Write-AuditLog INFO 'Dependency audit passed; no vulnerability suppressions or platform filters were applied.'
} catch {
    Write-AuditLog ERROR 'Dependency audit could not pass; do not merge until the failure is resolved.'
    exit 1
} finally {
    Pop-Location
}
