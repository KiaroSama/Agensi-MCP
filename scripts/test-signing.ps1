#requires -Version 7.4
param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][ValidateSet('windows-x64','linux-x64','macos-arm64','macos-x64')][string]$Platform,
    [Parameter(Mandatory)][string]$SourceCommit,
    [Parameter(Mandatory)][string]$Version
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'signing-common.ps1')
Initialize-SigningLog 'test-signing'
$fixture = Protect-SigningDirectory (Join-Path $script:SigningRoot ('.tmp/signature-controls-' + [guid]::NewGuid().ToString('N')))
$Path = [IO.Path]::GetFullPath($Path)
try {
    & (Join-Path $PSScriptRoot 'verify-release.ps1') -Path $Path -Platform $Platform -SourceCommit $SourceCommit -Version $Version
    $candidate = Join-Path $fixture ([IO.Path]::GetFileName($Path))
    Copy-Item -LiteralPath $Path -Destination $candidate
    Copy-Item -LiteralPath ($Path + '.sig') -Destination ($candidate + '.sig')
    $wrongKey = [Security.Cryptography.RSA]::Create(3072)
    try {
        $stream = [IO.File]::OpenRead($Path)
        try { $signature = [Convert]::FromBase64String([IO.File]::ReadAllText(($Path + '.sig'), [Text.UTF8Encoding]::new($false,$true)))
            if ($wrongKey.VerifyData($stream, $signature, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)) { throw 'Wrong detached public key accepted.' }
        } finally { $stream.Dispose() }
        if ($IsWindows) {
            $request = [Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=Wrong Signing Fixture', $wrongKey, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
            $wrong = $request.CreateSelfSigned([DateTimeOffset]::UtcNow.AddMinutes(-5), [DateTimeOffset]::UtcNow.AddDays(1))
            try { [IO.File]::WriteAllText((Join-Path $fixture 'wrong.pem'), $wrong.ExportCertificatePem(), [Text.UTF8Encoding]::new($false)) } finally { $wrong.Dispose() }
            # Call the native verifier directly: detached rejection must not hide a broken native oracle.
            $rejected = $false
            try { [AgensiAuthenticode]::Verify($candidate, (Join-Path $fixture 'wrong.pem'), (Join-Path $script:SigningRoot 'signing/root.pem')) } catch { $rejected = $true }
            if (-not $rejected) { throw 'Wrong native Authenticode signer accepted.' }
        }
    } finally { $wrongKey.Dispose() }
    if ($IsMacOS) {
        $rejectedRequirement = '=certificate leaf = H"' + ('0' * 40) + '"'
        $result = Invoke-SigningProcess /usr/bin/codesign @('--verify','--strict','-R',$rejectedRequirement,$candidate) -AllowFailure
        # Native exit3 means intact signed code rejected this valid requirement, not a parsing/file error.
        if ($result.ExitCode -ne 3) { throw 'Wrong native macOS signer must fail with an unmet requirement, not an operational or syntax error.' }
    }
    $stream = [IO.File]::Open($candidate, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try {
        $offset = 4096L
        if ($IsWindows) {
            $reader = [IO.BinaryReader]::new($stream, [Text.UTF8Encoding]::new($false), $true)
            try {
                $stream.Position = 0x3c; $pe = $reader.ReadUInt32()
                $stream.Position = $pe + 20; $size = $reader.ReadUInt16()
                $stream.Position = $pe + 24 + $size + 20; $offset = $reader.ReadUInt32() + 64L
            } finally { $reader.Dispose() }
        }
        if ($offset -ge $stream.Length) { throw 'Fixture executable has no covered mutation byte.' }
        $stream.Position = $offset; $value = $stream.ReadByte(); $stream.Position--; $stream.WriteByte($value -bxor 1)
    } finally { $stream.Dispose() }
    $rejected = $false
    try { & (Join-Path $PSScriptRoot 'verify-release.ps1') -Path $candidate -Platform $Platform -SourceCommit $SourceCommit -Version $Version } catch { $rejected = $true }
    if (-not $rejected) { throw 'Tampered executable accepted by detached verification.' }
    if ($IsWindows) {
        $rejected = $false
        try { [AgensiAuthenticode]::Verify($candidate, (Join-Path $script:SigningRoot 'signing/signer.pem'), (Join-Path $script:SigningRoot 'signing/root.pem')) } catch { $rejected = $true }
        if (-not $rejected) { throw 'Covered PE byte mutation accepted by native verification.' }
    } elseif ($IsMacOS) {
        $result = Invoke-SigningProcess /usr/bin/codesign @('--verify','--strict',$candidate) -AllowFailure
        if ($result.ExitCode -eq 0) { throw 'Covered Mach-O byte mutation accepted by native verification.' }
    }
    Write-SigningLog INFO 'Positive signature and exact identity passed; wrong public key, wrong Windows signer and covered-byte mutation controls rejected as applicable.'
} finally { Close-SigningLog }
