#requires -Version 7.4
param(
    [Parameter(Mandatory)][string]$Path,
    [ValidateSet('windows-x64','linux-x64','macos-arm64','macos-x64','detached')][string]$Platform = 'detached',
    [string]$SignaturePath,
    [string]$SigningDirectory,
    [string]$SourceCommit,
    [string]$Version
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'signing-common.ps1')
Initialize-SigningLog 'verify-release'
$key = $null
$temporary = $null
try {
    $Path = [IO.Path]::GetFullPath($Path)
    Assert-SigningPath $Path
    if (-not $SigningDirectory) { $SigningDirectory = Join-Path $script:SigningRoot 'signing' }
    $descriptor = Get-SigningDescriptor $SigningDirectory
    if (-not $SignaturePath) { $SignaturePath = $Path + '.sig' }
    Assert-SigningPath $SignaturePath
    if ((Get-Item -LiteralPath $SignaturePath).Length -gt 4096) { throw 'Oversized detached signature refused.' }
    $key = [Security.Cryptography.RSA]::Create()
    $key.ImportFromPem([IO.File]::ReadAllText((Join-Path $SigningDirectory 'public-key.pem'), [Text.UTF8Encoding]::new($false, $true)))
    $signature = [Convert]::FromBase64String([IO.File]::ReadAllText($SignaturePath, [Text.UTF8Encoding]::new($false, $true)).Trim())
    $stream = [IO.File]::OpenRead($Path)
    try {
        if (-not $key.VerifyData($stream, $signature, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)) { throw 'Detached signature verification failed.' }
    } finally { $stream.Dispose() }
    if ($Platform -eq 'windows-x64') {
        if (-not $IsWindows) { throw 'Native Windows signature verification requires Windows.' }
        if (-not ('AgensiAuthenticode' -as [type])) {
            $references = @((Get-ChildItem (Join-Path $PSHOME 'ref/*.dll')).FullName) + (Join-Path $PSHOME 'System.Security.Cryptography.Pkcs.dll')
            Add-Type -Path (Join-Path $PSScriptRoot 'WindowsAuthenticode.cs') -ReferencedAssemblies $references
        }
        [AgensiAuthenticode]::Verify($Path, (Join-Path $SigningDirectory 'signer.pem'), (Join-Path $SigningDirectory 'root.pem'))
    } elseif ($Platform.StartsWith('macos-')) {
        if (-not $IsMacOS) { throw 'Native macOS signature verification requires macOS.' }
        $requirement = 'certificate leaf = H"' + $descriptor.leaf_sha1 + '"'
        Invoke-SigningProcess /usr/bin/codesign @('--verify','--strict','-R',$requirement,$Path) | Out-Null
        $details = Invoke-SigningProcess /usr/bin/codesign @('-d','--verbose=4',$Path)
        if (($details.Stdout + $details.Stderr) -match 'Signature=adhoc' -or ($details.Stdout + $details.Stderr) -notmatch 'Authority=KiaroSama') { throw 'macOS signature is not the approved certificate-backed identity.' }
        $temporary = Protect-SigningDirectory (Join-Path $script:SigningRoot ('.tmp/verify-signature-' + [guid]::NewGuid().ToString('N')))
        $prefix = Join-Path $temporary 'certificate'
        Invoke-SigningProcess /usr/bin/codesign @('-d','--extract-certificates',$prefix,$Path) | Out-Null
        $leaf = [Security.Cryptography.X509Certificates.X509Certificate2]::new([IO.File]::ReadAllBytes($prefix + '0'))
        try { if ($leaf.GetCertHashString([Security.Cryptography.HashAlgorithmName]::SHA256).ToLowerInvariant() -cne $descriptor.leaf_sha256) { throw 'macOS signer pin mismatch.' } } finally { $leaf.Dispose() }
        Invoke-SigningProcess /usr/bin/security @('verify-cert','-c',(Join-Path $SigningDirectory 'signer.pem'),'-r',(Join-Path $SigningDirectory 'root.pem'),'-p','codeSign','-N','-L') | Out-Null
    }
    if ($Platform -ne 'detached') {
        if ($SourceCommit -cnotmatch '^[0-9a-f]{40}$' -or $Version -notmatch '^\d+\.\d+\.\d+$') { throw 'Expected release source commit and version are required.' }
        $result = Invoke-SigningProcess $Path @('doctor')
        $identity = $result.Stdout | ConvertFrom-Json
        if ($identity.publisher -cne 'KiaroSama' -or $identity.source_commit -cne $SourceCommit -or $identity.version -cne $Version) { throw 'Executable publisher, version or source identity mismatch.' }
    }
    Write-SigningLog INFO 'Signature, approved publisher pins and requested source identity verified; private trust only.'
} catch {
    Write-SigningLog ERROR 'Release verification failed; signature or identity is not accepted.'
    throw
} finally {
    if ($key) { $key.Dispose() }
    if ($temporary -and [IO.Directory]::Exists($temporary)) { [IO.Directory]::Delete($temporary, $true) }
    Close-SigningLog
}
