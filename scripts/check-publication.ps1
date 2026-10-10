$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $files = @(& git ls-files --cached --others --exclude-standard)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot enumerate publication files.' }
    $blocked = '(^|/)(\.ai|\.specify|specs|\.claude|\.codex|\.agents|\.cursor|\.kiro|node_modules|logs|\.state|\.cache|\.tmp|target|dist|bin|graphify-out|\.codebase-memory)(/|$)|(^|/)(secrets\.md|explain-AI\.md|\.ignoreme|config\.local\.json|Proxy list\.txt|\.env(\..*)?)$'
    $secret = '(gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,}|sk-ant-[A-Za-z0-9_-]{20,}|-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----|cs_live_[A-Za-z0-9]{20,})'
    $utf8 = [Text.UTF8Encoding]::new($false, $true)
    foreach ($name in $files) {
        if ($name -match $blocked) { throw "Private/generated publication path refused: $name" }
        $file = Join-Path $root $name
        if (-not (Test-Path $file -PathType Leaf)) { continue }
        if ((Get-Item -LiteralPath $file -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Linked publication file refused: $name" }
        $text = [IO.File]::ReadAllText($file, $utf8)
        if ($text -match $secret) { throw "Sensitive-pattern match: $name; value omitted." }
        if ($name -match '\.(rs|ps1)$' -and ($text -split "`n").Count -gt 800) { throw "Source line ceiling exceeded: $name" }
    }
    $bundle = $root
    $skill = Join-Path $bundle 'skills/agensi-operator'
    $main = [IO.File]::ReadAllText((Join-Path $skill 'SKILL.md'), $utf8).Replace("`r`n", "`n")
    if (-not $main.StartsWith("---`n") -or -not $main.Contains("name: agensi-operator`n")) { throw 'Invalid skill frontmatter.' }
    $native = [IO.File]::ReadAllText((Join-Path $root 'plugin.json'), $utf8) | ConvertFrom-Json
    if ($native.name -ne 'agensi-operator' -or @($native.PSObject.Properties.Name).Count -ne 2) { throw 'Invalid Antigravity root manifest.' }
    $resources = @(Get-ChildItem $skill -Recurse -File)
    foreach ($resource in $resources) {
        $relative = [IO.Path]::GetRelativePath($skill, $resource.FullName)
        $original = [IO.File]::ReadAllText($resource.FullName, $utf8)
        if ($original -match 'agensi[.]io/creators/') { throw 'Creator directory is not reusable operating guidance.' }
        foreach ($match in [regex]::Matches($original, '\]\(([^)]+)\)')) {
            $target = $match.Groups[1].Value
            if ($target -match '^(https?:|#)') { continue }
            $linked = [IO.Path]::GetFullPath((Join-Path $resource.DirectoryName ($target -split '#')[0]))
            if (-not $linked.StartsWith($skill + [IO.Path]::DirectorySeparatorChar) -or -not (Test-Path $linked)) { throw 'Broken/escaping skill reference.' }
        }
    }
    foreach ($client in @('claude', 'codex', 'cursor')) {
        $manifest = [IO.File]::ReadAllText((Join-Path $bundle ".$client-plugin/plugin.json"), $utf8) | ConvertFrom-Json
        if ($manifest.name -ne 'agensi-operator' -or $manifest.skills -ne './skills/' -or $manifest.license -ne 'GPL-3.0-only') { throw "Invalid $client manifest." }
    }
    Write-Output "Publication checks passed: $($files.Count) source files; references and client parity valid."
} finally { Pop-Location }
