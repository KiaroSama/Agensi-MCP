#requires -Version 7.4
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$utf8 = [Text.UTF8Encoding]::new($false, $true)
$root = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$owner = Join-Path $PSScriptRoot 'test-tls-proxy-ci.ps1'
$state = $env:AGENSI_TLS_CI_STATE
if (-not $IsWindows -or $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or
    -not $env:GITHUB_WORKSPACE -or [IO.Path]::GetFullPath($env:GITHUB_WORKSPACE) -cne $root -or
    -not $state -or -not [IO.Directory]::Exists($state) -or
    -not [IO.File]::Exists($owner) -or $env:AGENSI_TLS_CI_SCRIPT -cne $owner) {
    throw 'Invalid native fixture worker authority.'
}
function Assert-WorkerPath([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path)
    if ($full.StartsWith('\\?\')) { $full = $full.Substring(4) }
    $prefix = Join-Path $root '.tmp'
    if (-not $full.StartsWith($prefix + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Worker path escaped owned scratch.'
    }
    for ($item = $full; $item; $item = Split-Path $item -Parent) {
        if ((Test-Path -LiteralPath $item) -and ((Get-Item -LiteralPath $item -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Linked worker path refused.'
        }
        if ((Split-Path $item -Parent) -eq $item) { break }
    }
    return $full
}
$null = Assert-WorkerPath $state
if ((Get-Item -LiteralPath $owner -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
    throw 'Linked fixture owner refused.'
}
function Get-FrameLength([string]$Header) {
    if ($Header -cnotmatch '^[0-9]{4}$') { throw 'Invalid worker frame length.' }
    $length = [int]$Header
    if ($length -lt 1 -or $length -gt 8192) { throw 'Worker frame length outside bound.' }
    return $length
}
function Read-Request([byte[]]$Bytes, [uint32]$Expected) {
    if ($Bytes.Length -lt 1 -or $Bytes.Length -gt 8192) { throw 'Worker request outside frame bound.' }
    $document = [Text.Json.JsonDocument]::Parse($utf8.GetString($Bytes))
    try {
        if ($document.RootElement.ValueKind -ne [Text.Json.JsonValueKind]::Object) { throw 'Worker request must be an object.' }
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        foreach ($property in $document.RootElement.EnumerateObject()) {
            if (-not $names.Add($property.Name)) { throw 'Duplicate worker request key.' }
        }
        if ($names.Count -ne 5 -or @('id','action','root','binary','certificate').Where({ -not $names.Contains($_) }).Count) {
            throw 'Unexpected worker request schema.'
        }
        $request = ConvertFrom-Json -AsHashtable ($utf8.GetString($Bytes))
        if ($request.id -isnot [long] -or $request.id -lt 1 -or $request.id -gt [uint32]::MaxValue -or $request.id -ne $Expected -or
            $request.action -isnot [string] -or $request.action -cnotin @('Validate','Protect','Trust','RemoveCase')) {
            throw 'Invalid worker request identity or action.'
        }
        foreach ($key in @('root','binary','certificate')) {
            if ($null -ne $request[$key] -and ($request[$key] -isnot [string] -or $request[$key].Length -eq 0 -or $request[$key].Length -gt 2048)) {
                throw 'Invalid worker request path type.'
            }
        }
        $shape = switch ($request.action) {
            'Validate' { $null -eq $request.root -and $null -eq $request.binary -and $null -eq $request.certificate }
            'RemoveCase' { $null -ne $request.root -and $null -eq $request.binary -and $null -eq $request.certificate }
            'Protect' { $null -ne $request.root -and $null -ne $request.binary -and $null -eq $request.certificate }
            'Trust' { $null -ne $request.root -and $null -eq $request.binary -and $null -ne $request.certificate }
        }
        if (-not $shape) { throw 'Worker request path shape differs.' }
        return $request
    } finally { $document.Dispose() }
}
foreach ($header in @('0001','8192')) { $null = Get-FrameLength $header }
foreach ($header in @('0000','8193','001','abcd')) {
    $rejected = $false
    try { $null = Get-FrameLength $header } catch { $rejected = $true }
    if (-not $rejected) { throw 'Worker header negative failed.' }
}
$positive = '{"id":1,"action":"Validate","root":null,"binary":null,"certificate":null}'
$validRequests = @(
    $positive,
    '{"id":1,"action":"RemoveCase","root":"scratch","binary":null,"certificate":null}',
    '{"id":1,"action":"Protect","root":"scratch","binary":"binary","certificate":null}',
    '{"id":1,"action":"Trust","root":"scratch","binary":null,"certificate":"certificate"}'
)
foreach ($value in $validRequests) {
    $parsed = Read-Request ($utf8.GetBytes($value)) 1
    if ($parsed.id -ne 1) { throw 'Worker request positive failed.' }
}
$invalidRequests = @(
    $positive.Replace('"id":1','"id":2'), $positive.Replace('Validate','Execute'),
    $positive.Replace('"id":1','"id":0'), $positive.Replace('"id":1','"id":1.0'),
    $positive.Replace('"id":1','"id":"1"'), $positive.Replace('"id":1','"id":4294967296'),
    $positive.Replace('"root":null','"root":true'), $positive.Replace('"id":1','"id":1,"id":1'),
    $positive.Replace('"id":1','"id":1,"extra":null'), $positive.Replace(',"certificate":null',''),
    $positive.Replace('"root":null','"root":"unexpected"'),
    $validRequests[1].Replace('"root":"scratch"','"root":null'),
    $validRequests[1].Replace('"binary":null','"binary":"unexpected"'),
    $validRequests[2].Replace('"binary":"binary"','"binary":null'),
    $validRequests[2].Replace('"certificate":null','"certificate":"unexpected"'),
    $validRequests[3].Replace('"certificate":"certificate"','"certificate":null'),
    $validRequests[3].Replace('"binary":null','"binary":"unexpected"'),
    $validRequests[3].Replace('"root":"scratch"','"root":""'),
    $validRequests[3].Replace('"root":"scratch"', ('"root":"' + ('x' * 2049) + '"')),
    '{}', '{'
)
foreach ($value in $invalidRequests) {
    $rejected = $false
    try { $null = Read-Request ($utf8.GetBytes($value)) 1 } catch { $rejected = $true }
    if (-not $rejected) { throw 'Worker request negative failed.' }
}
foreach ($bytes in @(([byte[]]@(0xc3, 0x28)), ([byte[]]::new(8193)), ([byte[]]::new(0)))) {
    $rejected = $false
    try { $null = Read-Request $bytes 1 } catch { $rejected = $true }
    if (-not $rejected) { throw 'Worker byte request negative failed.' }
}
function Write-WorkerLine([string]$Line) {
    $bytes = $utf8.GetBytes($Line + "`n")
    $outputStream.Write($bytes, 0, $bytes.Length)
    $outputStream.Flush()
}
function Read-Exact([IO.Stream]$Stream, [int]$Length, [switch]$AllowEof) {
    if ($Length -lt 1 -or $Length -gt 8192) { throw 'Worker read length outside bound.' }
    $bytes = [byte[]]::new($Length)
    $offset = 0
    while ($offset -lt $Length) {
        $count = $Stream.Read($bytes, $offset, $Length - $offset)
        if ($count -eq 0) {
            if ($AllowEof -and $offset -eq 0) { return $null }
            throw 'Truncated worker frame.'
        }
        $offset += $count
    }
    return ,$bytes
}
$sample = $utf8.GetBytes($positive)
$frame = [IO.MemoryStream]::new()
try {
    $header = $utf8.GetBytes($sample.Length.ToString('D4'))
    $frame.Write($header, 0, $header.Length)
    $frame.Write($sample, 0, $sample.Length)
    $frame.Position = 0
    $length = Get-FrameLength ($utf8.GetString((Read-Exact $frame 4)))
    $null = Read-Request (Read-Exact $frame $length) 1
    if ($null -ne (Read-Exact $frame 4 -AllowEof)) { throw 'Worker clean EOF control failed.' }
} finally { $frame.Dispose() }
foreach ($case in @(
    @{ Bytes = [byte[]]::new(0); Length = 4; AllowEof = $false },
    @{ Bytes = $utf8.GetBytes('001'); Length = 4; AllowEof = $true },
    @{ Bytes = $sample[0..($sample.Length - 2)]; Length = $sample.Length; AllowEof = $false }
)) {
    $stream = [IO.MemoryStream]::new([byte[]]$case.Bytes)
    try {
        $rejected = $false
        try { $null = Read-Exact $stream $case.Length -AllowEof:$case.AllowEof } catch { $rejected = $true }
        if (-not $rejected) { throw 'Worker truncated frame negative failed.' }
    } finally { $stream.Dispose() }
}
$inputStream = [Console]::OpenStandardInput()
$outputStream = [Console]::OpenStandardOutput()
try {
    Import-Module NetSecurity -Global -ErrorAction Stop
    Write-WorkerLine 'READY'
    [uint32]$expected = 1
    while ($true) {
        $header = Read-Exact $inputStream 4 -AllowEof
        if ($null -eq $header) { break }
        $request = Read-Request (Read-Exact $inputStream (Get-FrameLength ($utf8.GetString($header)))) $expected
        foreach ($key in @('root','binary','certificate')) {
            if ($null -ne $request[$key]) { $null = Assert-WorkerPath $request[$key] }
        }
        try {
            & $owner -Action $request.action -CaseRoot $request.root -Binary $request.binary -Certificate $request.certificate -WorkerInvocation | Out-Null
        } catch {
            Write-WorkerLine ($expected.ToString() + ':ERR')
            throw 'Native fixture worker action failed.'
        }
        Write-WorkerLine ($expected.ToString() + ':OK')
        if ($expected -eq [uint32]::MaxValue) { throw 'Worker sequence exhausted.' }
        $expected++
    }
} catch {
    [Console]::Error.WriteLine('Native fixture worker failed; private payload omitted.')
    exit 1
} finally {
    $inputStream.Dispose()
    $outputStream.Dispose()
}
