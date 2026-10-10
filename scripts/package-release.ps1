param(
    [Parameter(Mandatory)][ValidateSet('windows-x64','linux-x64','macos-arm64','macos-x64')][string]$Platform,
    [string]$BinaryPath
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$windowsPackage = $Platform -eq 'windows-x64'
$executable = if ($windowsPackage) { 'agensi-mcp.exe' } else { 'agensi-mcp' }
if (-not $BinaryPath) { $BinaryPath = Join-Path $root "target/release/$executable" }
$BinaryPath = [IO.Path]::GetFullPath($BinaryPath)
if (-not (Test-Path -LiteralPath $BinaryPath -PathType Leaf)) { throw 'Built release executable is missing.' }
$dist = Join-Path $root 'dist'
[IO.Directory]::CreateDirectory($dist) | Out-Null
$stage = Join-Path $dist ('stage-' + [guid]::NewGuid().ToString('N'))
$folderName = "agensi-mcp-$Platform"
$payload = Join-Path $stage $folderName
$utf8 = [Text.UTF8Encoding]::new($false)
Push-Location $root
try {
    [IO.Directory]::CreateDirectory($payload) | Out-Null
    # Only committed public files enter a download; never copy a user's working installation.
    $sourceArchive = Join-Path $stage 'source.zip'
    & git archive --format=zip --output=$sourceArchive HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Cannot package committed source.' }
    Expand-Archive -LiteralPath $sourceArchive -DestinationPath $payload
    [IO.Directory]::CreateDirectory((Join-Path $payload 'bin')) | Out-Null
    Copy-Item -LiteralPath $BinaryPath -Destination (Join-Path $payload "bin/$executable")
    if (-not $windowsPackage) { & chmod +x (Join-Path $payload "bin/$executable"); if ($LASTEXITCODE -ne 0) { throw 'Cannot set executable permission.' } }
    $sha = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Cannot identify source commit.' }
    [IO.File]::WriteAllText((Join-Path $payload 'BUILD-INFO.txt'), "Platform: $Platform`nSource commit: $sha`n", $utf8)
    $foreignCwd = Join-Path $stage 'foreign-cwd'
    [IO.Directory]::CreateDirectory($foreignCwd) | Out-Null
    Push-Location $foreignCwd
    try {
        $output = & (Join-Path $payload "bin/$executable") status
        if ($LASTEXITCODE -ne 0) { throw 'Packaged executable failed startup.' }
        $status = ($output -join "`n") | ConvertFrom-Json
        if ($status.mode -ne 'disconnected' -or @($status.profiles).Count -ne 0) { throw 'Download must start disconnected without profiles.' }
    } finally { Pop-Location }
    # The smoke check created local files; they are not part of a distributable download.
    foreach ($relative in @('logs', '.management.lock')) {
        $generated = Join-Path $payload $relative
        if (Test-Path -LiteralPath $generated) { Remove-Item -LiteralPath $generated -Recurse -Force }
    }
    $forbidden = @('config.local.json','Proxy list.txt','.state','.cache','.tmp','target','node_modules')
    foreach ($relative in $forbidden) {
        if (Test-Path -LiteralPath (Join-Path $payload $relative)) { throw "Runtime/private payload refused: $relative" }
    }
    if ($windowsPackage) {
        $archive = Join-Path $dist "$folderName.zip"
        if (Test-Path -LiteralPath $archive) { throw 'Archive already exists; use an empty output directory.' }
        # Compress-Archive omits hidden manifests on Unix; .NET ZIP includes them on every host.
        [IO.Compression.ZipFile]::CreateFromDirectory($payload, $archive, [IO.Compression.CompressionLevel]::Optimal, $true)
    } else {
        $archive = Join-Path $dist "$folderName.tar.gz"
        if (Test-Path -LiteralPath $archive) { throw 'Archive already exists; use an empty output directory.' }
        & tar -czf $archive -C $stage $folderName
        if ($LASTEXITCODE -ne 0) { throw 'Cannot create native archive.' }
    }
    $verify = Join-Path $stage 'extracted'
    [IO.Directory]::CreateDirectory($verify) | Out-Null
    if ($windowsPackage) { Expand-Archive -LiteralPath $archive -DestinationPath $verify } else {
        & tar -xzf $archive -C $verify
        if ($LASTEXITCODE -ne 0) { throw 'Cannot verify extracted archive.' }
    }
    $extracted = Join-Path $verify $folderName
    if (-not (Test-Path -LiteralPath (Join-Path $extracted '.claude-plugin/plugin.json'))) { throw 'Archive lost hidden plugin manifests.' }
    $originalHash = (Get-FileHash -LiteralPath $BinaryPath -Algorithm SHA256).Hash
    $extractedHash = (Get-FileHash -LiteralPath (Join-Path $extracted "bin/$executable") -Algorithm SHA256).Hash
    if ($originalHash -ne $extractedHash) { throw 'Archive changed executable bytes.' }
    Push-Location $foreignCwd
    try {
        $status = (& (Join-Path $extracted "bin/$executable") status | Out-String) | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0 -or $status.mode -ne 'disconnected') { throw 'Extracted download failed startup.' }
    } finally { Pop-Location }
    $hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText(($archive + '.sha256'), "$hash  $([IO.Path]::GetFileName($archive))`n", $utf8)
    Write-Output "Packaged and startup-verified: $([IO.Path]::GetFileName($archive))"
} finally {
    Pop-Location
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
}
