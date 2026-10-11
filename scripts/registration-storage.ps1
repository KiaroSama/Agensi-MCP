#requires -Version 7.0
# Cooperative multi-file registration: validate and stage all files before publishing.
# This does not claim atomicity against process termination or noncooperating writers.
function Write-RegistrationLog {
    param([ValidateSet('INFO','WARNING','ERROR','DEBUG')][string]$Level, [string]$Message)
    [Console]::Error.WriteLine(('{0} [{1}] {2}' -f [DateTimeOffset]::UtcNow.ToString('o'), $Level, $Message))
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
function Get-RegistrationSnapshot {
    param([string]$Path)
    $pathValue = [IO.Path]::GetFullPath($Path)
    $bytes = Read-RegistrationBytes $pathValue
    $value = @{}
    if ($null -ne $bytes) {
        try {
            $text = [Text.UTF8Encoding]::new($false, $true).GetString($bytes).TrimStart([char]0xfeff)
            $value = ConvertFrom-Json -InputObject $text -AsHashtable -Depth 100
        } catch { throw 'Registration config must contain valid UTF-8 JSON.' }
    }
    if (-not ($value -is [System.Collections.IDictionary])) { throw 'Registration config is not a JSON object.' }
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
            Publish-RegistrationFile $change.Stage $change.Path
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
