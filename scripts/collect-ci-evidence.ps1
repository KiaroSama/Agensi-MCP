$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
function Write-EvidenceLog {
    param([ValidateSet('INFO', 'WARNING', 'ERROR', 'DEBUG')][string]$Level, [string]$Message)
    [Console]::Error.WriteLine(('[{0:yyyy-MM-dd HH:mm:ss} UTC] [{1}] [ci-evidence] {2}' -f [DateTime]::UtcNow, $Level, $Message))
}
$root = Split-Path $PSScriptRoot -Parent
$target = Join-Path $root '.tmp/ci-review'
$utf8 = [Text.UTF8Encoding]::new($false)
Push-Location $root
try {
    New-Item $target -ItemType Directory -Force | Out-Null
    Write-EvidenceLog INFO 'Collecting bounded public CI evidence; no runtime state is exported.'
    $head = (& git rev-parse HEAD)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot identify tested source.' }
    [IO.File]::WriteAllText((Join-Path $target 'source-commit.txt'), $head + "`n", $utf8)
    foreach ($name in @('ci-native-tests.log', 'output-pressure.json')) {
        $path = Join-Path $root ".tmp/$name"
        if (Test-Path -LiteralPath $path) { Copy-Item -LiteralPath $path -Destination $target }
    }
    foreach ($name in @('agensi-mcp', 'check-run')) {
        $binary = Join-Path $root "target/debug/$name"
        if (Test-Path -LiteralPath $binary) {
            Copy-Item -LiteralPath $binary -Destination $target
            $hash = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
            [IO.File]::WriteAllText((Join-Path $target "$name.sha256"), "$hash  $name`n", $utf8)
        }
    }
    # Only committed public source is archived; ignored state, logs, private hooks and credentials are excluded.
    & git archive --format=tar --prefix=source/ "--output=$(Join-Path $target 'source.tar')" HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Cannot collect committed public source.' }
    # This final diagnostic step never commits, pushes, changes tested binaries or grants a PASS.
    # The native job's independent formatting gate remains blocking. Only the proposed diff is exported.
    & cargo +1.99.0 fmt --all
    if ($LASTEXITCODE -ne 0) { Write-EvidenceLog ERROR 'Cannot produce formatting diagnostics.'; exit 1 }
    $patch = Join-Path $target 'format.patch'
    & git diff --binary "--output=$patch"
    if ($LASTEXITCODE -ne 0) { throw 'Cannot collect formatting diff.' }
    Write-EvidenceLog INFO 'Validation evidence collected; consult actual job results before integration.'
} finally { Pop-Location }
