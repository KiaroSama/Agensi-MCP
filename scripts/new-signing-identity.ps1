#requires -Version 7.4
param([Parameter(Mandatory)][switch]$Create)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'signing-common.ps1')
Initialize-SigningLog 'signing-identity'
$rootKey = $leafKey = $rootCertificate = $leafCertificate = $null
try {
    if (-not $Create) { throw 'Explicit -Create approval is required.' }
    $private = Protect-SigningDirectory (Join-Path $script:SigningRoot '.state/signing')
    $public = Join-Path $script:SigningRoot 'signing'
    Assert-SigningPath $public
    if ([IO.Directory]::Exists($public) -or [IO.File]::Exists((Join-Path $private 'leaf.pfx')) -or [IO.File]::Exists((Join-Path $private 'root.pfx'))) { throw 'Signing identity already exists; rotation is a separate approved operation.' }
    $registry = Join-Path $script:SigningRoot 'secrets.md'
    Assert-SigningPrivateFile $registry
    $password = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32)).ToLowerInvariant()
    $rootPassword = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32)).ToLowerInvariant()
    $rootKey = [Security.Cryptography.RSA]::Create(3072)
    $leafKey = [Security.Cryptography.RSA]::Create(3072)
    $hash = [Security.Cryptography.HashAlgorithmName]::SHA256
    $padding = [Security.Cryptography.RSASignaturePadding]::Pkcs1
    $rootRequest = [Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=KiaroSama Private Signing Root', $rootKey, $hash, $padding)
    $rootRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($true, $true, 0, $true))
    $rootRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new([Security.Cryptography.X509Certificates.X509KeyUsageFlags]::KeyCertSign -bor [Security.Cryptography.X509Certificates.X509KeyUsageFlags]::CrlSign, $true))
    $rootRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509SubjectKeyIdentifierExtension]::new($rootRequest.PublicKey, $false))
    $now = [DateTimeOffset]::UtcNow
    $rootCertificate = $rootRequest.CreateSelfSigned($now.AddMinutes(-5), $now.AddYears(5))
    $leafRequest = [Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=KiaroSama', $leafKey, $hash, $padding)
    $leafRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($false, $false, 0, $true))
    $leafRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new([Security.Cryptography.X509Certificates.X509KeyUsageFlags]::DigitalSignature, $true))
    $usage = [Security.Cryptography.OidCollection]::new()
    $usage.Add([Security.Cryptography.Oid]::new('1.3.6.1.5.5.7.3.3')) | Out-Null
    $leafRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]::new($usage, $true))
    $leafRequest.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509SubjectKeyIdentifierExtension]::new($leafRequest.PublicKey, $false))
    $issued = $leafRequest.Create($rootCertificate, $now.AddMinutes(-5), $now.AddYears(1), [Security.Cryptography.RandomNumberGenerator]::GetBytes(16))
    try { $leafCertificate = [Security.Cryptography.X509Certificates.RSACertificateExtensions]::CopyWithPrivateKey($issued, $leafKey) } finally { $issued.Dispose() }
    $chain = [Security.Cryptography.X509Certificates.X509Certificate2Collection]::new()
    $chain.Add($leafCertificate) | Out-Null
    $rootPublic = [Security.Cryptography.X509Certificates.X509Certificate2]::new($rootCertificate.RawData)
    try { $chain.Add($rootPublic) | Out-Null; $leafBytes = $chain.Export([Security.Cryptography.X509Certificates.X509ContentType]::Pkcs12, $password) } finally { $rootPublic.Dispose() }
    $rootBytes = $rootCertificate.Export([Security.Cryptography.X509Certificates.X509ContentType]::Pkcs12, $rootPassword)
    # Persist recoverable passwords before creating encrypted key files, including on partial failure.
    $utf8 = [Text.UTF8Encoding]::new($false)
    $custody = "`n## Agensi private release signing`nUpdated UTC: $($now.ToString('o'))`nStatus: pending creation; preserve this record and any partial encrypted files on failure.`nPublisher: KiaroSama`nTrust: private self-signed; no public CA or notarization`nLeaf encrypted PFX: .state/signing/leaf.pfx`nRoot encrypted PFX: .state/signing/root.pfx (local custody only; never CI)`nAGENSI_SIGNING_PASSWORD: $password`nAGENSI_SIGNING_ROOT_PASSWORD: $rootPassword`nCI input AGENSI_SIGNING_PFX_B64: encode only the encrypted leaf PFX at transfer; never upload this registry.`nPurpose: native private release signatures; changing either certificate requires reviewed pins and CI redeployment.`n"
    $registryStream = [IO.File]::Open($registry, [IO.FileMode]::Append, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $registryStream.Write($utf8.GetBytes($custody)); $registryStream.Flush($true) } finally { $registryStream.Dispose() }
    foreach ($item in @(@('leaf.pfx', $leafBytes), @('root.pfx', $rootBytes))) {
        $path = Join-Path $private $item[0]
        $stream = [IO.File]::Open($path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $stream.Write([byte[]]$item[1]); $stream.Flush($true) } finally { $stream.Dispose() }
        if (-not $IsWindows) { [IO.File]::SetUnixFileMode($path, [IO.UnixFileMode]::UserRead -bor [IO.UnixFileMode]::UserWrite) }
    }
    $readback = [Security.Cryptography.X509Certificates.X509Certificate2Collection]::new()
    try {
        $readback.Import([IO.File]::ReadAllBytes((Join-Path $private 'leaf.pfx')), $password, [Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet)
        $privateLeaves = @($readback | Where-Object HasPrivateKey)
        if ($privateLeaves.Count -ne 1 -or $privateLeaves[0].Thumbprint -cne $leafCertificate.Thumbprint) { throw 'Encrypted leaf custody readback failed.' }
        $readbackRoot = [Security.Cryptography.X509Certificates.X509Certificate2]::new([IO.File]::ReadAllBytes((Join-Path $private 'root.pfx')), $rootPassword, [Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet)
        try { if (-not $readbackRoot.HasPrivateKey -or $readbackRoot.Thumbprint -cne $rootCertificate.Thumbprint) { throw 'Encrypted root custody readback failed.' } } finally { $readbackRoot.Dispose() }
    } finally { foreach ($certificate in $readback) { $certificate.Dispose() } }
    [IO.Directory]::CreateDirectory($public) | Out-Null
    function Write-PublicSigningFile([string]$Name, [string]$Text) {
        $stream = [IO.File]::Open((Join-Path $public $Name), [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $stream.Write($utf8.GetBytes($Text + "`n")); $stream.Flush($true) } finally { $stream.Dispose() }
    }
    Write-PublicSigningFile 'root.pem' $rootCertificate.ExportCertificatePem()
    Write-PublicSigningFile 'signer.pem' $leafCertificate.ExportCertificatePem()
    Write-PublicSigningFile 'public-key.pem' $leafKey.ExportSubjectPublicKeyInfoPem()
    $descriptor = [ordered]@{ schema = 1; publisher = 'KiaroSama'; trust = 'private-self-signed'; leaf_sha256 = $leafCertificate.GetCertHashString($hash).ToLowerInvariant(); root_sha256 = $rootCertificate.GetCertHashString($hash).ToLowerInvariant(); leaf_sha1 = $leafCertificate.Thumbprint.ToLowerInvariant(); not_before_utc = $leafCertificate.NotBefore.ToUniversalTime().ToString('o'); not_after_utc = $leafCertificate.NotAfter.ToUniversalTime().ToString('o'); notarized = $false; trusted_timestamp = $false }
    Write-PublicSigningFile 'identity.json' ($descriptor | ConvertTo-Json)
    Get-SigningDescriptor | Out-Null
    $registryStream = [IO.File]::Open($registry, [IO.FileMode]::Append, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $registryStream.Write($utf8.GetBytes("Status: creation complete; encrypted custody readback and public pins verified.`n")); $registryStream.Flush($true) } finally { $registryStream.Dispose() }
    Write-SigningLog INFO 'Approved private identity created; encrypted local custody and public pins saved. No key store or remote secret was modified.'
} catch {
    Write-SigningLog ERROR 'Identity creation failed; preserve private custody for review. Values and exception payload omitted.'
    throw 'Private identity creation failed; review safe signing status without exposing custody values.'
} finally {
    foreach ($value in @($leafCertificate, $rootCertificate, $leafKey, $rootKey)) { if ($value) { $value.Dispose() } }
    $password = $rootPassword = $custody = $leafBytes = $rootBytes = $null
    Close-SigningLog
}
