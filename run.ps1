param([switch]$Status, [switch]$Register, [switch]$Preview, [switch]$SignIn, [string]$ImportProxyList)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$node = Get-Command node -ErrorAction SilentlyContinue
if (-not $node) { Write-Error 'Node.js 24 or newer is required.'; exit 1 }
if (-not (Test-Path (Join-Path $root 'node_modules/mcp-remote/dist/proxy.js'))) {
    Write-Error 'Dependencies missing. Run npm install in this folder first.'; exit 1
}
$manager = Join-Path $root 'src/manage.mjs'
$defaultList = Join-Path $root 'Proxy list.txt'
function Invoke-Manager([string[]]$Arguments) {
    $output = & $node.Source $manager @Arguments
    if ($LASTEXITCODE -ne 0) { throw 'Management command failed.' }
    $output | ConvertFrom-Json
}
function Write-Painted([string]$Text, [string]$Code, [ConsoleColor]$Fallback = 'White', [switch]$NoNewline) {
    if ($Host.UI.SupportsVirtualTerminal -and -not [Console]::IsOutputRedirected) {
        Write-Host ("`e[" + $Code + 'm' + $Text + "`e[0m") -NoNewline:$NoNewline
    } else { Write-Host $Text -ForegroundColor $Fallback -NoNewline:$NoNewline }
}
function Show-Profiles($state) {
    Write-Painted 'Connection: ' '1' White -NoNewline
    $label = $state.mode.ToUpperInvariant()
    $color = if ($state.mode -eq 'proxy') { 'Green' } elseif ($state.mode -eq 'direct') { 'Yellow' } else { 'DarkGray' }
    Write-Host $label -ForegroundColor $color
    if (@($state.profiles).Count -eq 0) { Write-Host '  No proxy profiles. Import a list or add one.' -ForegroundColor DarkGray }
    for ($i = 0; $i -lt @($state.profiles).Count; $i++) {
        $profile = $state.profiles[$i]
        $active = if ($state.mode -eq 'proxy' -and $state.selected -eq $profile.name) { ' [active]' } else { '' }
        $number = $i + 1
        Write-Painted "  $number. " '38;5;117' Cyan -NoNewline
        Write-Host ($profile.name + ' - ' + $profile.protocol + '://' + $profile.host + ':' + $profile.port + $active)
    }
}
function Show-Menu($state) {
    Write-Host ''
    Write-Painted 'Agensi MCP Main menu:' '1;38;5;117' Cyan
    $items = @('Import proxy list from file', 'Select proxy profile', 'Add proxy profile', 'Direct connection (explicit)', 'Disconnect', 'Register Desktop and Code', 'Sign in (copy link to external browser)')
    for ($i = 0; $i -lt $items.Count; $i++) {
        $number = $i + 1
        Write-Painted "  $number. " '38;5;117' Cyan -NoNewline
        Write-Host $items[$i] -NoNewline
        if ($number -eq 1) { Write-Painted ' [1]' '92' Green -NoNewline }
        Write-Host ''
    }
    Write-Host ''
    Write-Host ('Connection: ' + $state.mode.ToUpperInvariant() + ' | Proxy: ' + $state.selected + ' | Saved proxies: ' + @($state.profiles).Count)
    Write-Host ''
}
function Import-List([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path)) { $Path = $defaultList }
    $Path = $Path.Trim().Trim('"').Trim("'")
    if (-not [IO.Path]::IsPathRooted($Path)) { $Path = Join-Path $root $Path }
    $result = Invoke-Manager @('import', $Path)
    Write-Host ("Imported: " + $result.added + "; skipped duplicates: " + $result.skipped + '. Route unchanged.') -ForegroundColor Green
}
try {
    if ($Status) { Invoke-Manager @('status') | ConvertTo-Json -Depth 5; exit 0 }
    if ($Register) { & (Join-Path $root 'scripts/register.ps1'); exit $LASTEXITCODE }
    if ($ImportProxyList) { Import-List $ImportProxyList; exit 0 }
    if ($SignIn) { & $node.Source (Join-Path $root 'src/bridge.mjs') --signin; exit $LASTEXITCODE }
    if ($Preview) { Show-Menu (Invoke-Manager @('status')); exit 0 }
    while ($true) {
        $state = Invoke-Manager @('status')
        Show-Menu $state
        Write-Painted 'Selection ' '1' White -NoNewline
        Write-Painted '[1] ' '92' Green -NoNewline
        Write-Painted 'quit=exit' '38;5;32' Blue -NoNewline
        Write-Host ': ' -NoNewline
        $choice = Read-Host
        if (-not $choice) { $choice = '1' }
        try {
            switch ($choice.Trim().ToLowerInvariant()) {
                '1' { Import-List (Read-Host "Proxy list path [$defaultList]") }
                '2' {
                    Show-Profiles $state
                    $pick = Read-Host 'Profile number or name (back=b)'
                    if ($pick -eq 'b') { break }
                    $index = 0
                    if ([int]::TryParse($pick, [ref]$index) -and $index -ge 1 -and $index -le @($state.profiles).Count) {
                        $pick = $state.profiles[$index - 1].name
                    }
                    Invoke-Manager @('select', $pick) | Out-Null
                    Write-Host 'Proxy selected. Reconnect Agensi in the client.' -ForegroundColor Green
                }
                '3' {
                    $name = Read-Host 'Profile name'
                    Write-Host 'URL: scheme://username:password@host:port (encode credentials).' -ForegroundColor Cyan
                    $secret = Read-Host 'Proxy URL (hidden)' -AsSecureString
                    $ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secret)
                    try {
                        $url = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr)
                        $output = $url | & $node.Source $manager add $name --stdin
                        if ($LASTEXITCODE -ne 0) { throw 'Proxy was not saved.' }
                        Write-Host 'Proxy profile added.' -ForegroundColor Green
                    } finally { $url = $null; [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr) }
                }
                '4' {
                    if ((Read-Host 'Allow direct Agensi connections? [y/N]') -match '^(y|yes)$') {
                        Invoke-Manager @('direct') | Out-Null
                        Write-Host 'Direct mode selected. Reconnect Agensi.' -ForegroundColor Yellow
                    }
                }
                '5' { Invoke-Manager @('disconnect') | Out-Null; Write-Host 'Disconnected.' -ForegroundColor Yellow }
                '6' { & (Join-Path $root 'scripts/register.ps1') }
                '7' {
                    Write-Host 'Copy the complete sign-in URL into your external browser. Keep this window open.' -ForegroundColor Cyan
                    & $node.Source (Join-Path $root 'src/bridge.mjs') --signin
                    if ($LASTEXITCODE -ne 0) { throw 'Sign-in did not finish. No session claimed.' }
                }
                'exit' { exit 0 }
                '0' { exit 0 }
                default { Write-Host 'Enter a menu number from 1 to 7, or exit.' -ForegroundColor Red }
            }
        } catch { Write-Host $_.Exception.Message -ForegroundColor Red }
    }
} catch { Write-Host $_.Exception.Message -ForegroundColor Red; exit 1 }
