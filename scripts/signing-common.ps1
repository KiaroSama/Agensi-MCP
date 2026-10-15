#requires -Version 7.4
$script:SigningRoot = Split-Path $PSScriptRoot -Parent
$script:SigningLog = $null
function Initialize-SigningLog([string]$Component) {
    try {
        $directory = Join-Path $script:SigningRoot 'logs'
        Assert-SigningPath $directory
        [IO.Directory]::CreateDirectory($directory) | Out-Null
        $name = '{0}_{1}_UTC_{2}.log' -f $Component, [DateTime]::UtcNow.ToString('yyyy-MM-dd_HH-mm-ss'), [guid]::NewGuid().ToString('N')
        $stream = [IO.File]::Open((Join-Path $directory $name), [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
        $script:SigningLog = [IO.StreamWriter]::new($stream, [Text.UTF8Encoding]::new($false))
        $script:SigningLog.AutoFlush = $true
    } catch { [Console]::Error.WriteLine('Signing file logging unavailable; safe status uses stderr.') }
}
function Write-SigningLog([string]$Level, [string]$Message) {
    $entry = '[{0:yyyy-MM-dd HH:mm:ss} UTC] [{1}] [signing] {2}' -f [DateTime]::UtcNow, $Level, $Message
    if ($script:SigningLog) { try { $script:SigningLog.WriteLine($entry) } catch { Close-SigningLog } }
    [Console]::Error.WriteLine($entry)
}
function Close-SigningLog {
    if ($script:SigningLog) { $script:SigningLog.Dispose(); $script:SigningLog = $null }
}
function Assert-SigningPath([string]$Path) {
    $current = [IO.Path]::GetFullPath($Path)
    while ($current) {
        if ((Test-Path -LiteralPath $current) -and ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Linked signing path refused.' }
        $current = [IO.Path]::GetDirectoryName($current)
    }
}
function Assert-SigningPrivateFile([string]$Path) {
    Assert-SigningPath $Path
    if (-not [IO.File]::Exists($Path)) { throw 'Required private signing file is missing.' }
    if ($IsWindows) {
        $owner = [Security.Principal.WindowsIdentity]::GetCurrent().User
        $acl = Get-Acl -LiteralPath $Path
        if ($acl.GetOwner([Security.Principal.SecurityIdentifier]) -ne $owner) { throw 'Private signing file must be owner-controlled.' }
        foreach ($rule in $acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])) {
            if ($rule.AccessControlType -eq 'Allow' -and $rule.IdentityReference -ne $owner) { throw 'Private signing file grants access to another identity.' }
        }
    } elseif (([int][IO.File]::GetUnixFileMode($Path) -band 0x3f) -ne 0) { throw 'Private signing file must exclude group and other access (0600).' }
}
function Protect-SigningDirectory([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path)
    Assert-SigningPath $full
    [IO.Directory]::CreateDirectory($full) | Out-Null
    if ($IsWindows) {
        $owner = [Security.Principal.WindowsIdentity]::GetCurrent().User
        $acl = [Security.AccessControl.DirectorySecurity]::new()
        $acl.SetOwner($owner)
        $acl.SetAccessRuleProtection($true, $false)
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($owner, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
        Set-Acl -LiteralPath $full -AclObject $acl
    } else { [IO.File]::SetUnixFileMode($full, [IO.UnixFileMode]::UserRead -bor [IO.UnixFileMode]::UserWrite -bor [IO.UnixFileMode]::UserExecute) }
    return $full
}
function Invoke-SigningProcess {
    param([string]$Program, [string[]]$Arguments, [string]$InputText = '', [int]$Seconds = 30, [switch]$AllowFailure)
    $start = [Diagnostics.ProcessStartInfo]::new($Program)
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false, $true)
    $start.StandardErrorEncoding = [Text.UTF8Encoding]::new($false, $true)
    $start.WorkingDirectory = $script:SigningRoot
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    foreach ($name in @('AGENSI_SIGNING_PFX_B64', 'AGENSI_SIGNING_PASSWORD')) { $start.Environment.Remove($name) | Out-Null }
    if ($Seconds -lt 1 -or $Seconds -gt 120 -or $InputText.Length -gt 4096) { throw 'Invalid signing process bounds.' }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    $started = $false
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $activity = 0L
    try {
        $started = $process.Start()
        if (-not $started) { throw 'Signing subprocess did not start.' }
        $readers = @($process.StandardOutput, $process.StandardError)
        $buffers = @([char[]]::new(4096), [char[]]::new(4096))
        $texts = @([Text.StringBuilder]::new(), [Text.StringBuilder]::new())
        $reads = @($readers[0].ReadAsync($buffers[0], 0, 4096), $readers[1].ReadAsync($buffers[1], 0, 4096))
        $write = $process.StandardInput.WriteAsync($InputText)
        while ($true) {
            if ($write -and $write.IsCompleted) {
                $write.GetAwaiter().GetResult()
                $process.StandardInput.Close()
                $write = $null
                $activity = $watch.ElapsedMilliseconds
            }
            for ($index = 0; $index -lt 2; $index++) {
                if ($reads[$index] -and $reads[$index].IsCompleted) {
                    $count = $reads[$index].GetAwaiter().GetResult()
                    if ($texts[0].Length + $texts[1].Length + $count -gt 1MB) { throw 'Signing diagnostics exceeded their output bound.' }
                    if ($count -eq 0) { $reads[$index] = $null } else {
                        $texts[$index].Append($buffers[$index], 0, $count) | Out-Null
                        $reads[$index] = $readers[$index].ReadAsync($buffers[$index], 0, 4096)
                        $activity = $watch.ElapsedMilliseconds
                    }
                }
            }
            if ($process.HasExited -and -not $write -and -not $reads[0] -and -not $reads[1]) { break }
            if ($watch.Elapsed.TotalSeconds -ge $Seconds -or $watch.ElapsedMilliseconds - $activity -ge 15000) { throw 'Signing subprocess exceeded its wall or idle deadline.' }
            $pending = @($write, $reads[0], $reads[1] | Where-Object { $null -ne $_ })
            if ($pending.Count) { [Threading.Tasks.Task]::WaitAny([Threading.Tasks.Task[]]$pending, 50) | Out-Null } else { $process.WaitForExit(50) | Out-Null }
        }
        $result = [pscustomobject]@{ ExitCode = $process.ExitCode; Stdout = $texts[0].ToString(); Stderr = $texts[1].ToString() }
        if ($result.ExitCode -ne 0 -and -not $AllowFailure) { throw 'Signing subprocess failed; private diagnostics omitted.' }
        return $result
    } finally {
        if ($started -and -not $process.HasExited) {
            $process.Kill($true)
            if (-not $process.WaitForExit(5000)) { [Console]::Error.WriteLine('Signing subprocess cleanup deadline exceeded.') }
        }
        $process.Dispose()
    }
}
function Get-SigningDescriptor([string]$Directory = (Join-Path $script:SigningRoot 'signing')) {
    Assert-SigningPath $Directory
    foreach ($name in @('identity.json', 'signer.pem', 'root.pem', 'public-key.pem')) { Assert-SigningPath (Join-Path $Directory $name) }
    $descriptor = [IO.File]::ReadAllText((Join-Path $Directory 'identity.json'), [Text.UTF8Encoding]::new($false, $true)) | ConvertFrom-Json
    if ($descriptor.schema -ne 1 -or $descriptor.publisher -cne 'KiaroSama' -or $descriptor.trust -cne 'private-self-signed' -or $descriptor.leaf_sha256 -notmatch '^[0-9a-f]{64}$' -or $descriptor.root_sha256 -notmatch '^[0-9a-f]{64}$') { throw 'Invalid approved signing descriptor.' }
    $utf8 = [Text.UTF8Encoding]::new($false, $true)
    $leaf = [Security.Cryptography.X509Certificates.X509Certificate2]::CreateFromPem([IO.File]::ReadAllText((Join-Path $Directory 'signer.pem'), $utf8))
    $root = [Security.Cryptography.X509Certificates.X509Certificate2]::CreateFromPem([IO.File]::ReadAllText((Join-Path $Directory 'root.pem'), $utf8))
    $chain = [Security.Cryptography.X509Certificates.X509Chain]::new()
    $publicKey = [Security.Cryptography.RSA]::Create()
    $leafKey = $null
    try {
        if ($leaf.GetCertHashString([Security.Cryptography.HashAlgorithmName]::SHA256).ToLowerInvariant() -cne $descriptor.leaf_sha256 -or $root.GetCertHashString([Security.Cryptography.HashAlgorithmName]::SHA256).ToLowerInvariant() -cne $descriptor.root_sha256 -or $leaf.Thumbprint.ToLowerInvariant() -cne $descriptor.leaf_sha1 -or $leaf.Subject -cne 'CN=KiaroSama' -or $descriptor.notarized -ne $false -or $descriptor.trusted_timestamp -ne $false) { throw 'Signing certificate pin or trust description mismatch.' }
        $basic = @($root.Extensions | Where-Object { $_ -is [Security.Cryptography.X509Certificates.X509BasicConstraintsExtension] })
        $usage = @($leaf.Extensions | Where-Object { $_ -is [Security.Cryptography.X509Certificates.X509KeyUsageExtension] })
        $eku = @($leaf.Extensions | Where-Object { $_ -is [Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension] })
        if ($basic.Count -ne 1 -or -not $basic[0].CertificateAuthority -or $usage.Count -ne 1 -or $usage[0].KeyUsages -ne [Security.Cryptography.X509Certificates.X509KeyUsageFlags]::DigitalSignature -or $eku.Count -ne 1 -or @($eku[0].EnhancedKeyUsages | Where-Object Value -eq '1.3.6.1.5.5.7.3.3').Count -ne 1) { throw 'Invalid private code-signing certificate constraints.' }
        $chain.ChainPolicy.TrustMode = [Security.Cryptography.X509Certificates.X509ChainTrustMode]::CustomRootTrust
        $chain.ChainPolicy.CustomTrustStore.Add($root)
        $chain.ChainPolicy.RevocationMode = [Security.Cryptography.X509Certificates.X509RevocationMode]::NoCheck
        $chain.ChainPolicy.DisableCertificateDownloads = $true
        $chain.ChainPolicy.ApplicationPolicy.Add([Security.Cryptography.Oid]::new('1.3.6.1.5.5.7.3.3'))
        if (-not $chain.Build($leaf)) { throw 'Private signing chain is invalid or expired.' }
        $publicKey.ImportFromPem([IO.File]::ReadAllText((Join-Path $Directory 'public-key.pem'), [Text.UTF8Encoding]::new($false, $true)))
        $leafKey = [Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPublicKey($leaf)
        if (-not $leafKey -or $publicKey.KeySize -lt 3072 -or [Convert]::ToBase64String($publicKey.ExportSubjectPublicKeyInfo()) -cne [Convert]::ToBase64String($leafKey.ExportSubjectPublicKeyInfo())) { throw 'Pinned signing public key does not match the certificate.' }
    } finally { if ($leafKey) { $leafKey.Dispose() }; $publicKey.Dispose(); $chain.Dispose(); $leaf.Dispose(); $root.Dispose() }
    return $descriptor
}
