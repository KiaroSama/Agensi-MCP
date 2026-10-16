#requires -Version 7.4
param(
    [Parameter(Mandatory)][string]$Path,
    [ValidateSet('windows-x64','linux-x64','macos-arm64','macos-x64','detached')][string]$Platform = 'detached',
    [string]$PfxPath,
    [string]$SourceCommit,
    [string]$Version,
    [switch]$Preflight
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'signing-common.ps1')
Initialize-SigningLog 'sign-release'
$certificates = [Security.Cryptography.X509Certificates.X509Certificate2Collection]::new()
$key = $null
$temporary = $null
$keychain = $null
$stage = 'input-validation'
Write-SigningLog INFO 'Signing stage=input-validation.'
$password = $env:AGENSI_SIGNING_PASSWORD
$encoded = $env:AGENSI_SIGNING_PFX_B64
# Children never inherit the encrypted key or password, even during doctor execution.
$env:AGENSI_SIGNING_PASSWORD = $null
$env:AGENSI_SIGNING_PFX_B64 = $null
try {
    $Path = [IO.Path]::GetFullPath($Path)
    Assert-SigningPath $Path
    if (-not $Preflight -and (-not [IO.File]::Exists($Path) -or [IO.File]::Exists($Path + '.sig'))) { throw 'Signing requires an existing file without a prior detached signature.' }
    $stage = 'public-descriptor'
    Write-SigningLog INFO 'Signing stage=public-descriptor.'
    $descriptor = Get-SigningDescriptor
    Write-SigningLog INFO 'Signing stage=public-descriptor-complete.'
    if ($password -notmatch '^[0-9a-fA-F]{64}$') { throw 'Approved signing password input is missing or invalid.' }
    if ($PfxPath) {
        Assert-SigningPrivateFile $PfxPath
        $pfx = [IO.File]::ReadAllBytes($PfxPath)
    } else {
        if (-not $encoded -or $encoded.Length -gt 48000) { throw 'Approved encrypted signing input is missing or oversized.' }
        $pfx = [Convert]::FromBase64String($encoded)
    }
    if ($IsMacOS -and $env:RUNNER_ENVIRONMENT -cne 'github-hosted') { throw 'macOS private signing is restricted to the approved disposable hosted runner.' }
    # macOS lacks EphemeralKeySet: .NET owns a temporary keychain and removes it on certificate disposal.
    $storage = if ($IsMacOS) { [Security.Cryptography.X509Certificates.X509KeyStorageFlags]::DefaultKeySet } else { [Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet }
    $stage = if ($IsMacOS) { 'pfx-import-temporary-keychain' } else { 'pfx-import-ephemeral' }
    Write-SigningLog INFO ('Signing stage=' + $stage + '.')
    $certificates.Import($pfx, $password, $storage)
    Write-SigningLog INFO 'Signing stage=pfx-import-complete.'
    $private = @($certificates | Where-Object HasPrivateKey)
    if ($private.Count -ne 1 -or $private[0].GetCertHashString([Security.Cryptography.HashAlgorithmName]::SHA256).ToLowerInvariant() -cne $descriptor.leaf_sha256) { throw 'Signing input is not the exact approved leaf identity.' }
    $certificate = $private[0]
    $key = [Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($certificate)
    if ($Preflight) { Write-SigningLog INFO 'Encrypted leaf input, private chain and pinned identity ready; no file signed or installed.'; return }
    if ($Platform -ne 'detached') {
        if ($SourceCommit -cnotmatch '^[0-9a-f]{40}$' -or $Version -notmatch '^\d+\.\d+\.\d+$') { throw 'Release source commit and version are required before signing.' }
        $stage = 'unsigned-doctor'
        Write-SigningLog INFO 'Signing stage=unsigned-doctor.'
        $identity = (Invoke-SigningProcess $Path @('doctor')).Stdout | ConvertFrom-Json
        Write-SigningLog INFO 'Signing stage=unsigned-doctor-complete.'
        if ($identity.publisher -cne 'KiaroSama' -or $identity.source_commit -cne $SourceCommit -or $identity.version -cne $Version) { throw 'Unsigned executable identity does not match the release.' }
    }
    if ($Platform -eq 'windows-x64') {
        if (-not $IsWindows) { throw 'Windows signing requires Windows.' }
        # The OS may report an untrusted private root after writing a valid signature. Never accept that status alone.
        $stage = 'windows-authenticode'
        Write-SigningLog INFO 'Signing stage=windows-authenticode.'
        Set-AuthenticodeSignature -LiteralPath $Path -Certificate $certificate -HashAlgorithm SHA256 -IncludeChain All | Out-Null
        Write-SigningLog INFO 'Signing stage=windows-authenticode-complete.'
    } elseif ($Platform.StartsWith('macos-')) {
        if (-not $IsMacOS) { throw 'macOS signing requires macOS.' }
        $temporary = Protect-SigningDirectory (Join-Path $script:SigningRoot ('.tmp/signature-' + [guid]::NewGuid().ToString('N')))
        $keychain = Join-Path $temporary 'release.keychain-db'
        $pfxFile = Join-Path $temporary 'leaf.pfx'
        [IO.File]::WriteAllBytes($pfxFile, $pfx)
        [IO.File]::SetUnixFileMode($pfxFile, [IO.UnixFileMode]::UserRead -bor [IO.UnixFileMode]::UserWrite)
        $keychainPassword = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
        function Invoke-KeychainCommand([string[]]$Tokens) {
            $script:stage = 'mac-keychain-' + $Tokens[0]
            Write-SigningLog INFO ('Signing stage=' + $script:stage + '.')
            $line = ($Tokens | ForEach-Object { '"' + $_.Replace('\','\\').Replace('"','\"') + '"' }) -join ' '
            if ($line.Length -gt 4000 -or $line -match '[\r\n]') { throw 'Invalid protected keychain command.' }
            Invoke-SigningProcess /usr/bin/security @('-i') ($line + "`n") | Out-Null
        }
        Invoke-KeychainCommand @('create-keychain','-p',$keychainPassword,$keychain)
        Invoke-KeychainCommand @('unlock-keychain','-p',$keychainPassword,$keychain)
        Invoke-KeychainCommand @('import',$pfxFile,'-k',$keychain,'-P',$password,'-T','/usr/bin/codesign')
        Invoke-KeychainCommand @('set-key-partition-list','-S','apple-tool:,apple:,codesign:','-s','-k',$keychainPassword,$keychain)
        $stage = 'mac-codesign'
        Write-SigningLog INFO 'Signing stage=mac-codesign.'
        $native = Invoke-SigningProcess /usr/bin/codesign @('--force','--keychain',$keychain,'--sign',$descriptor.leaf_sha1,'--timestamp=none',$Path) -AllowFailure
        if ($native.ExitCode -ne 0) {
            $classification = 'unknown'
            foreach ($known in @('errSecInternalComponent','errSecCSSignatureFailed','errSecCSUntrustedCert','errSecCSNoTrustSettings','unable to build chain','no identity found','specified item could not be found','User interaction is not allowed')) {
                if ($native.Stderr.Contains($known, [StringComparison]::OrdinalIgnoreCase)) { $classification = $known; break }
            }
            # Only codesign receives this public diagnostic route, never security -i or its protected input.
            $detail = $native.Stderr
            foreach ($sensitive in @($encoded, $password, $keychainPassword, $Path, $keychain, $temporary, $script:SigningRoot)) {
                if ($sensitive) { $detail = $detail.Replace($sensitive, '[REDACTED]') }
            }
            $detail = [regex]::Replace($detail, '[\p{Cc}\p{Cf}]', ' ')
            if ($detail.Length -gt 2048) { $detail = $detail.Substring(0, 2048) }
            Write-SigningLog ERROR ('Native codesign failed; exit={0}; classification={1}; public_detail={2}' -f $native.ExitCode, $classification, $detail)
            throw 'Native certificate-backed signing failed.'
        }
        Write-SigningLog INFO 'Signing stage=mac-codesign-complete.'
    }
    $stage = 'detached-signature'
    Write-SigningLog INFO 'Signing stage=detached-signature.'
    $stream = [IO.File]::OpenRead($Path)
    try { $signature = $key.SignData($stream, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1) } finally { $stream.Dispose() }
    $stream = [IO.File]::Open(($Path + '.sig'), [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        $text = [Text.UTF8Encoding]::new($false).GetBytes([Convert]::ToBase64String($signature) + "`n")
        $stream.Write($text); $stream.Flush($true)
    } finally { $stream.Dispose() }
    $stage = 'signed-verification'
    Write-SigningLog INFO 'Signing stage=signed-verification.'
    & (Join-Path $PSScriptRoot 'verify-release.ps1') -Path $Path -Platform $Platform -SourceCommit $SourceCommit -Version $Version
    Write-SigningLog INFO 'Release bytes signed and cryptographically verified with approved private identity; no public trust or timestamp claimed.'
} catch {
    Write-SigningLog ERROR ('Release signing failed; stage={0}; exception={1}; hresult={2}; line={3}; payload omitted.' -f $stage, $_.Exception.GetType().Name, $_.Exception.HResult, $_.InvocationInfo.ScriptLineNumber)
    throw 'Release signing failed; private process diagnostics and key inputs omitted.'
} finally {
    if ($key) { $key.Dispose() }
    foreach ($certificate in $certificates) { $certificate.Dispose() }
    try {
        if ($keychain -and [IO.File]::Exists($keychain)) { Invoke-SigningProcess /usr/bin/security @('delete-keychain',$keychain) | Out-Null }
        if ($temporary -and [IO.Directory]::Exists($temporary)) { [IO.Directory]::Delete($temporary, $true) }
    } finally {
        $env:AGENSI_SIGNING_PASSWORD = $password
        $env:AGENSI_SIGNING_PFX_B64 = $encoded
        $password = $encoded = $pfx = $keychainPassword = $null
        Close-SigningLog
    }
}
