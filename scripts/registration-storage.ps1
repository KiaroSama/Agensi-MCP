#requires -Version 7.0
# Cooperative multi-file registration: validate and stage all files before publishing.
# This does not claim atomicity against process termination or noncooperating writers.
function Initialize-RegistrationLog {
    param([string]$Root)
    $script:RegistrationLog = $null
    try {
        $directory = Join-Path $Root 'logs'
        Assert-RegistrationPath (Join-Path $directory 'registration.log')
        [IO.Directory]::CreateDirectory($directory) | Out-Null
        $stamp = [DateTime]::UtcNow.ToString('yyyy-MM-dd_HH-mm-ss')
        $path = Join-Path $directory ('register_' + $stamp + '_UTC_' + [guid]::NewGuid().ToString('N') + '.log')
        Write-RegistrationStage $path ([byte[]]::new(0))
        $script:RegistrationLog = [IO.StreamWriter]::new($path, $true, [Text.UTF8Encoding]::new($false))
        $script:RegistrationLog.AutoFlush = $true
    } catch {
        [Console]::Error.WriteLine('File logging unavailable; registration records use stderr.')
    }
}
function Close-RegistrationLog {
    if ($script:RegistrationLog) { $script:RegistrationLog.Dispose(); $script:RegistrationLog = $null }
}
function Write-RegistrationLog {
    param([ValidateSet('INFO','WARNING','ERROR','DEBUG')][string]$Level, [string]$Message)
    $safe = [regex]::Replace($Message, '[\x00-\x1f\x7f]', ' ')
    $entry = '[{0:yyyy-MM-dd HH:mm:ss} UTC] [{1}] [registration] {2}' -f [DateTime]::UtcNow, $Level, $safe
    if ($script:RegistrationLog) {
        try { $script:RegistrationLog.WriteLine($entry) } catch { Close-RegistrationLog }
    }
    [Console]::Error.WriteLine($entry)
}
function Assert-RegistrationPath {
    param([string]$Path)
    $current = [IO.Path]::GetFullPath($Path)
    $first = $true
    while ($current) {
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Linked registration path refused.' }
            if ($first -and $item.PSIsContainer) { throw 'Registration target must be a regular file.' }
        }
        $first = $false
        $current = [IO.Path]::GetDirectoryName($current)
    }
}
function Read-RegistrationBytes {
    param([string]$Path)
    Assert-RegistrationPath $Path
    if (-not [IO.File]::Exists($Path)) { return $null }
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $memory = [IO.MemoryStream]::new()
    try {
        $limit = 32MB
        if ($stream.Length -gt $limit) { throw 'Registration config exceeds its size limit.' }
        $buffer = [byte[]]::new(8192)
        while (($count = $stream.Read($buffer, 0, $buffer.Length)) -gt 0) {
            if ($memory.Length + $count -gt $limit) { throw 'Registration config exceeds its size limit.' }
            $memory.Write($buffer, 0, $count)
        }
        return ,$memory.ToArray()
    } finally { $stream.Dispose(); $memory.Dispose() }
}
function Test-RegistrationBytesEqual {
    param($Left, $Right)
    if ($null -eq $Left -or $null -eq $Right) { return ($null -eq $Left -and $null -eq $Right) }
    return $Left.Length -eq $Right.Length -and [Convert]::ToBase64String($Left) -ceq [Convert]::ToBase64String($Right)
}
function Convert-RegistrationToken {
    param([Newtonsoft.Json.Linq.JToken]$Token)
    if ($Token -is [Newtonsoft.Json.Linq.JObject]) {
        $value = [Collections.Hashtable]::new([StringComparer]::Ordinal)
        foreach ($property in $Token.Properties()) { $value[$property.Name] = Convert-RegistrationToken $property.Value }
        return $value
    }
    if ($Token -is [Newtonsoft.Json.Linq.JArray]) {
        $value = [object[]]::new($Token.Count)
        for ($index = 0; $index -lt $Token.Count; $index++) { $value[$index] = Convert-RegistrationToken $Token[$index] }
        return ,$value
    }
    return $Token.Value
}
function Get-RegistrationSnapshot {
    param([string]$Path)
    $pathValue = [IO.Path]::GetFullPath($Path)
    $bytes = Read-RegistrationBytes $pathValue
    $value = @{}
    if ($null -ne $bytes) {
        try {
            $text = [Text.UTF8Encoding]::new($false, $true).GetString($bytes).TrimStart([char]0xfeff)
            if ([string]::IsNullOrWhiteSpace($text)) { throw 'Empty existing configuration.' }
            $settings = [Newtonsoft.Json.JsonSerializerSettings]::new()
            $settings.DateParseHandling = [Newtonsoft.Json.DateParseHandling]::None
            $settings.TypeNameHandling = [Newtonsoft.Json.TypeNameHandling]::None
            $settings.MetadataPropertyHandling = [Newtonsoft.Json.MetadataPropertyHandling]::Ignore
            $settings.MaxDepth = 100
            $token = [Newtonsoft.Json.JsonConvert]::DeserializeObject($text, $settings)
        } catch { throw 'Registration config must contain valid UTF-8 JSON.' }
        if (-not ($token -is [Newtonsoft.Json.Linq.JObject])) { throw 'Registration config is not a JSON object.' }
        $value = Convert-RegistrationToken $token
    }
    return [pscustomobject]@{ Path = $pathValue; Bytes = $bytes; Value = $value; NewBytes = $null; Stage = $null; Backup = $null }
}
function Write-RegistrationStage {
    param([string]$Path, [byte[]]$Bytes)
    $file = [IO.File]::Open($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        if ($IsWindows) {
            # Protect the empty file before any possibly sensitive config bytes are written.
            $file.Dispose()
            $identity = [Security.Principal.WindowsIdentity]::GetCurrent().User
            $security = [Security.AccessControl.FileSecurity]::new()
            $security.SetOwner($identity)
            $security.SetAccessRuleProtection($true, $false)
            $security.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($identity, [Security.AccessControl.FileSystemRights]::FullControl, [Security.AccessControl.AccessControlType]::Allow))
            Set-Acl -LiteralPath $Path -AclObject $security
            $file = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::None)
        }
        $file.Write($Bytes, 0, $Bytes.Length)
        $file.Flush($true)
    } finally { $file.Dispose() }
}
function Publish-RegistrationFile {
    param([string]$Stage, [string]$Path)
    Assert-RegistrationPath $Path
    if ([IO.File]::Exists($Path)) {
        # Replace preserves the destination's security metadata on Windows.
        # A typed .NET null avoids PowerShell converting a bare $null to an empty path.
        [IO.File]::Replace($Stage, $Path, [NullString]::Value, $false)
    } else { [IO.File]::Move($Stage, $Path, $false) }
}
function Invoke-RegistrationTransaction {
    param([object[]]$Changes)
    $locks = [Collections.Generic.List[IDisposable]]::new()
    $published = [Collections.Generic.List[object]]::new()
    $keepBackups = $false
    $phase = 'locking'
    foreach ($change in $Changes) {
        if ($null -eq $change.NewBytes) { $change.NewBytes = $change.Bytes }
        if ($null -ne $change.NewBytes -and $change.NewBytes.Length -gt 32MB) { throw 'Registration output exceeds its size limit.' }
    }
    try {
        foreach ($change in @($Changes | Sort-Object Path)) {
            Assert-RegistrationPath $change.Path
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($change.Path)) | Out-Null
            $lockPath = $change.Path + '.agensi-registration.lock'
            Assert-RegistrationPath $lockPath
            $locks.Add([IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None))
        }
        $phase = 'staging'
        foreach ($change in $Changes) {
            if (-not (Test-RegistrationBytesEqual (Read-RegistrationBytes $change.Path) $change.Bytes)) { throw 'Registration settings changed concurrently; nothing published.' }
            if (Test-RegistrationBytesEqual $change.NewBytes $change.Bytes) { continue }
            $id = [guid]::NewGuid().ToString('N')
            $change.Stage = $change.Path + '.agensi-' + $id + '.tmp'
            Write-RegistrationStage $change.Stage $change.NewBytes
            if ($null -ne $change.Bytes) {
                $change.Backup = $change.Path + '.agensi-' + $id + '.recovery'
                Write-RegistrationStage $change.Backup $change.Bytes
            }
        }
        $phase = 'publishing'
        foreach ($change in $Changes) {
            if (-not (Test-RegistrationBytesEqual (Read-RegistrationBytes $change.Path) $change.Bytes)) { throw 'Registration settings changed concurrently during publication.' }
            if (Test-RegistrationBytesEqual $change.NewBytes $change.Bytes) { continue }
            try {
                Publish-RegistrationFile $change.Stage $change.Path
            } catch {
                # ReplaceFile may change paths before reporting an error; original recovery must survive.
                try {
                    $current = Read-RegistrationBytes $change.Path
                    if (Test-RegistrationBytesEqual $current $change.NewBytes) { $published.Add($change) }
                    elseif (-not (Test-RegistrationBytesEqual $current $change.Bytes)) { $keepBackups = $true }
                } catch { $keepBackups = $true }
                throw
            }
            $published.Add($change)
        }
        $phase = 'verifying'
        foreach ($change in $Changes) {
            if (-not (Test-RegistrationBytesEqual (Read-RegistrationBytes $change.Path) $change.NewBytes)) { throw 'Registration read-back verification failed.' }
        }
        Write-RegistrationLog INFO 'All requested registration targets were saved and verified.'
    } catch {
        # Report bounded structural diagnostics, never exception payloads or config contents.
        $failureType = $_.Exception.GetBaseException().GetType().Name
        $failureLine = $_.InvocationInfo.ScriptLineNumber
        Write-RegistrationLog ERROR ('Registration failed during ' + $phase + ' (' + $failureType + ', script line ' + $failureLine + '); payload omitted.')
        Write-RegistrationLog WARNING 'Registration did not complete; restoring only unchanged files written by this operation.'
        for ($index = $published.Count - 1; $index -ge 0; $index--) {
            $change = $published[$index]
            try {
                if (-not (Test-RegistrationBytesEqual (Read-RegistrationBytes $change.Path) $change.NewBytes)) { throw 'A concurrent writer changed a published target.' }
                if ($null -eq $change.Bytes) { [IO.File]::Delete($change.Path) }
                else { Publish-RegistrationFile $change.Backup $change.Path }
                if (-not (Test-RegistrationBytesEqual (Read-RegistrationBytes $change.Path) $change.Bytes)) { throw 'Restoration verification failed.' }
            } catch {
                $keepBackups = $true
                Write-RegistrationLog ERROR ('Automatic restoration refused or failed; preserve the private .recovery file beside: ' + $change.Path)
            }
        }
        throw 'Registration failed. Review the logged target status; no success was claimed.'
    } finally {
        try {
            foreach ($change in $Changes) {
                if ($change.Stage -and [IO.File]::Exists($change.Stage)) { [IO.File]::Delete($change.Stage) }
                if (-not $keepBackups -and $change.Backup -and [IO.File]::Exists($change.Backup)) { [IO.File]::Delete($change.Backup) }
            }
        } finally {
            foreach ($lock in $locks) { $lock.Dispose() }
        }
    }
}
