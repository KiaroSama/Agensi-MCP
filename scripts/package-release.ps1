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
$extension = if ($windowsPackage) { '.zip' } else { '.tar.gz' }
$archiveName = $folderName + $extension
$archive = Join-Path $stage $archiveName
$finalArchive = Join-Path $dist $archiveName
$finalChecksum = $finalArchive + '.sha256'
$publishedArchive = $false
$publishedChecksum = $false
$hash = $null
$publishedSignatures = @{}
$signatureHashes = @{}
. (Join-Path $PSScriptRoot 'signing-common.ps1')
$version = [regex]::Match([IO.File]::ReadAllText((Join-Path $root 'Cargo.toml'), $utf8), '(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"').Groups[1].Value
Push-Location $root
try {
    foreach ($path in @($finalArchive, $finalChecksum, ($finalArchive + '.sig'), ($finalChecksum + '.sig'))) { if (Test-Path -LiteralPath $path) { throw 'Release output already exists; use an empty output directory.' } }
    $sha = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Cannot identify source commit.' }
    & (Join-Path $PSScriptRoot 'verify-release.ps1') -Path $BinaryPath -Platform $Platform -SourceCommit $sha -Version $version
    [IO.Directory]::CreateDirectory($payload) | Out-Null
    # Only committed public files enter a download; never copy a user's working installation.
    $sourceArchive = Join-Path $stage 'source.zip'
    & git archive --format=zip --output=$sourceArchive HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Cannot package committed source.' }
    Expand-Archive -LiteralPath $sourceArchive -DestinationPath $payload
    [IO.Directory]::CreateDirectory((Join-Path $payload 'bin')) | Out-Null
    Copy-Item -LiteralPath $BinaryPath -Destination (Join-Path $payload "bin/$executable")
    Copy-Item -LiteralPath ($BinaryPath + '.sig') -Destination (Join-Path $payload "bin/$executable.sig")
    if (-not $windowsPackage) { & chmod +x (Join-Path $payload "bin/$executable"); if ($LASTEXITCODE -ne 0) { throw 'Cannot set executable permission.' } }
    $sha = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Cannot identify source commit.' }
    [IO.File]::WriteAllText((Join-Path $payload 'BUILD-INFO.txt'), "Platform: $Platform`nPublisher: KiaroSama`nVersion: $version`nSource commit: $sha`nTrust: private self-signed; not publicly trusted or notarized; no trusted timestamp`n", $utf8)
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
    foreach ($relative in @('logs', '.management.lock', '.runtime.lock')) {
        $generated = Join-Path $payload $relative
        if (Test-Path -LiteralPath $generated) { Remove-Item -LiteralPath $generated -Recurse -Force }
    }
    $forbidden = @('config.local.json','Proxy list.txt','.state','.cache','.tmp','target','node_modules')
    foreach ($relative in $forbidden) {
        if (Test-Path -LiteralPath (Join-Path $payload $relative)) { throw "Runtime/private payload refused: $relative" }
    }
    if ($windowsPackage) {
        # Compress-Archive omits hidden manifests on Unix; .NET ZIP includes them on every host.
        [IO.Compression.ZipFile]::CreateFromDirectory($payload, $archive, [IO.Compression.CompressionLevel]::Optimal, $true)
    } else {
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
    & (Join-Path $PSScriptRoot 'verify-release.ps1') -Path (Join-Path $extracted "bin/$executable") -Platform $Platform -SigningDirectory (Join-Path $extracted 'signing') -SourceCommit $sha -Version $version
    Push-Location $foreignCwd
    try {
        $status = (& (Join-Path $extracted "bin/$executable") status | Out-String) | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0 -or $status.mode -ne 'disconnected') { throw 'Extracted download failed startup.' }
    } finally { Pop-Location }
    $hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    $checksumText = "$hash  $archiveName`n"
    [IO.File]::WriteAllText(($archive + '.sha256'), $checksumText, $utf8)
    foreach ($path in @($archive, ($archive + '.sha256'))) {
        & (Join-Path $PSScriptRoot 'sign-release.ps1') -Path $path
        $signatureHashes[$path + '.sig'] = (Get-FileHash -LiteralPath ($path + '.sig') -Algorithm SHA256).Hash
    }
    if (Test-Path -LiteralPath $finalArchive) { throw 'Release archive destination appeared concurrently.' }
    if (Test-Path -LiteralPath $finalChecksum) { throw 'Release checksum destination appeared concurrently.' }
    # Both files are verified before no-overwrite publication; this is not crash-atomic pair publication.
    [IO.File]::Move($archive, $finalArchive, $false)
    $publishedArchive = $true
    [IO.File]::Move(($archive + '.sha256'), $finalChecksum, $false)
    $publishedChecksum = $true
    foreach ($source in $signatureHashes.Keys) {
        $destination = Join-Path $dist ([IO.Path]::GetFileName($source))
        [IO.File]::Move($source, $destination, $false)
        $publishedSignatures[$destination] = $signatureHashes[$source]
    }
    Write-Output "Packaged and startup-verified: $archiveName"
} catch {
    # Never remove preexisting or externally replaced outputs during handled-error compensation.
    try {
        foreach ($path in $publishedSignatures.Keys) { if ([IO.File]::Exists($path) -and (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ceq $publishedSignatures[$path]) { [IO.File]::Delete($path) } }
        if ($publishedChecksum -and [IO.File]::Exists($finalChecksum) -and [IO.File]::ReadAllText($finalChecksum, $utf8) -ceq $checksumText) { [IO.File]::Delete($finalChecksum) }
        if ($publishedArchive -and [IO.File]::Exists($finalArchive) -and (Get-FileHash -LiteralPath $finalArchive -Algorithm SHA256).Hash.ToLowerInvariant() -ceq $hash) { [IO.File]::Delete($finalArchive) }
    } catch { [Console]::Error.WriteLine('Release-output compensation incomplete; inspect owned outputs before retrying.') }
    throw
} finally {
    Pop-Location
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
}
