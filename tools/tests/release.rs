#[allow(dead_code)]
#[path = "../check/policy.rs"]
mod policy;

#[allow(dead_code)]
#[path = "../check/process.rs"]
mod process;
#[allow(dead_code)]
mod runner_fixture;

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Output,
    time::Duration,
};

struct ScriptInstallation {
    _directory: tempfile::TempDir,
    root: PathBuf,
    foreign: PathBuf,
    bootstrap: PathBuf,
}
impl ScriptInstallation {
    fn new() -> Self {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"));
        fs::create_dir_all(source.join(".tmp")).expect("Project scratch");
        let directory = tempfile::Builder::new()
            .prefix("release scripts-é-")
            .tempdir_in(source.join(".tmp"))
            .expect("Owned fixture");
        let root = directory.path().join("installation with spaces-é");
        let foreign = directory.path().join("foreign checkout-é");
        fs::create_dir_all(root.join("scripts")).expect("Script fixture");
        fs::create_dir_all(&foreign).expect("Foreign fixture");
        for name in [
            "build.ps1",
            "build.sh",
            "audit-dependencies.ps1",
            "package-release.ps1",
            "collect-ci-evidence.ps1",
            "signing-common.ps1",
        ] {
            fs::copy(
                source.join("scripts").join(name),
                root.join("scripts").join(name),
            )
            .expect("Copy script under test");
        }
        let bootstrap = root.join(if cfg!(windows) {
            "check-run.exe"
        } else {
            "check-run"
        });
        fs::copy(env!("CARGO_BIN_EXE_check-run"), &bootstrap).expect("Trusted bootstrap");
        Self {
            _directory: directory,
            root,
            foreign,
            bootstrap,
        }
    }
    fn powershell(&self, body: &str) -> Output {
        let script = self.root.join("fixture.ps1");
        fs::write(&script, body).expect("UTF8 fixture script");
        let command = [
            OsString::from("pwsh"),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-File".into(),
            script.into_os_string(),
            self.root.clone().into_os_string(),
            self.foreign.clone().into_os_string(),
        ];
        runner_fixture::capture_fixture_with_input(
            &self.bootstrap,
            &self.foreign,
            &command,
            b"",
            &runner_fixture::fixture_environment(&self.root),
            Duration::from_secs(20),
        )
    }
}
fn assert_script_success(output: Output) {
    assert!(
        output.status.success(),
        "Script fixture failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_matching_release() {
    assert!(policy::validate("2.1.0", "v2.1.0", "refs/heads/main", true).is_ok());
    let expanded = policy::expand_options(vec![
        "--manifest=path with spaces/Cargo.toml".into(),
        "--tag=v2.1.0".into(),
    ]);
    assert_eq!(
        expanded,
        vec![
            std::ffi::OsString::from("--manifest"),
            "path with spaces/Cargo.toml".into(),
            "--tag".into(),
            "v2.1.0".into()
        ]
    );
}
#[test]
fn test_artifacts_on_feature_branch() {
    assert!(policy::validate("2.1.0", "", "refs/heads/feature", true).is_ok());
}
#[test]
fn test_local_validation() {
    assert!(policy::validate("2.1.0", "v2.1.0", "", false).is_ok());
}
#[test]
fn test_different_version() {
    assert!(policy::validate("2.1.0", "v99.99.99", "refs/heads/main", true).is_err());
}
#[test]
fn test_unprefixed_tag() {
    assert!(policy::validate("2.1.0", "2.1.0", "refs/heads/main", true).is_err());
}
#[test]
fn test_shell_metacharacters() {
    assert!(policy::validate("2.1.0", "v2.1.0;echo forbidden", "refs/heads/main", true).is_err());
}
#[test]
fn test_nonmain_release() {
    assert!(policy::validate("2.1.0", "v2.1.0", "refs/heads/feature", true).is_err());
}
#[test]
fn reviewed_signing_branch_is_exact_and_never_a_generic_bypass() {
    let allowed = [
        "refs/heads/fix/private-signed-release",
        "KiaroSama/Agensi-MCP",
        "KiaroSama",
        "workflow_dispatch",
    ];
    assert!(policy::reviewed_signing_branch(
        allowed[0], allowed[1], allowed[2], allowed[3]
    ));
    for (index, rejected) in [
        (0, "refs/heads/feature/unreviewed"),
        (1, "other/Agensi-MCP"),
        (2, "other"),
        (3, "pull_request"),
    ] {
        let mut candidate = allowed;
        candidate[index] = rejected;
        assert!(!policy::reviewed_signing_branch(
            candidate[0],
            candidate[1],
            candidate[2],
            candidate[3]
        ));
    }
}

#[test]
fn test_prerelease_policy() {
    assert!(policy::validate("2.1.0-beta", "v2.1.0-beta", "refs/heads/main", true).is_err());
}

#[test]
fn build_and_audit_scripts_contain_child_scratch_and_restore_environment() {
    let fixture = ScriptInstallation::new();
    assert_script_success(fixture.powershell(r#"
param($Root, $Foreign)
$ErrorActionPreference = 'Stop'
$utf8 = [Text.UTF8Encoding]::new($false)
$names = @('CARGO_HOME','CARGO_TARGET_DIR','CARGO_BUILD_JOBS','TEMP','TMP','TMPDIR','AGENSI_SOURCE_COMMIT')
foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, ('caller-'+$name), 'Process') }
$global:FixtureCalls = [Collections.Generic.List[object]]::new()
$global:FixtureFail = 'none'
$global:FixtureRoot = $Root
function global:cargo {
    $record = @{ cwd = $PWD.Path; argv = @($args) }
    foreach ($name in $names) { $record[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
    $global:FixtureCalls.Add($record)
    if ($global:FixtureFail -eq 'throw') { throw 'Controlled Cargo failure.' }
    $global:LASTEXITCODE = if ($global:FixtureFail -eq 'exit') { 23 } else { 0 }
    if ($args -contains 'install') {
        $executable = Join-Path $global:FixtureRoot ('.cache/audit-tools/bin/cargo-audit' + $(if ($IsWindows) { '.exe' } else { '' }))
        Set-Item -Path ('Function:global:'+$executable) -Value { $global:FixtureCalls.Add(@{ audit = $true; cwd = $PWD.Path }); $global:LASTEXITCODE = 0 }
    }
}
[IO.Directory]::CreateDirectory((Join-Path $Root 'target/debug')) | Out-Null
[IO.File]::WriteAllBytes((Join-Path $Root 'target/debug/agensi-mcp.exe'), [byte[]]@(1,2,3))
foreach ($script in @('build.ps1','audit-dependencies.ps1')) {
    foreach ($fail in @('none','throw','exit')) {
        $global:FixtureFail = $fail
        $failed = $false
        $global:LASTEXITCODE = 0
        try {
            if ($script -eq 'build.ps1') { & (Join-Path $Root "scripts/$script") -Debug } else { & (Join-Path $Root "scripts/$script") }
            $failed = $LASTEXITCODE -ne 0
        } catch { $failed = $true }
        if ($failed -ne ($fail -ne 'none')) { throw 'Failure result was not preserved.' }
        if ($PWD.Path -cne $Foreign) { throw 'Caller directory changed.' }
        foreach ($name in $names) {
            if ([Environment]::GetEnvironmentVariable($name,'Process') -cne ('caller-'+$name)) { throw 'Caller environment was not restored.' }
        }
    }
}
if ($global:FixtureCalls.Count -ne 7) { throw 'Expected build/install success, exception/nonzero failures and audit observations.' }
foreach ($record in $global:FixtureCalls) {
    if ($record.cwd -cne $Root) { throw 'Child ran from a foreign directory.' }
    if ($record.audit) { continue }
    foreach ($name in @('TEMP','TMP','TMPDIR','CARGO_HOME','CARGO_TARGET_DIR')) {
        if (-not $record[$name].StartsWith($Root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or -not [IO.Directory]::Exists($record[$name])) { throw 'Child scratch escaped or was not created.' }
    }
    if ($record.CARGO_BUILD_JOBS -ne '4') { throw 'Worker ceiling changed.' }
}
'Contained child environment and restored caller state on success/failure.'
"#));
}

#[test]
#[cfg(unix)]
fn bash_build_restores_sourced_caller_and_contains_child_environment() {
    let fixture = ScriptInstallation::new();
    let script = fixture.root.join("fixture.sh");
    fs::write(&script, r#"#!/usr/bin/env bash
set -uo pipefail
root="$1"
foreign="$2"
export CARGO_HOME=caller-home CARGO_TARGET_DIR=caller-target CARGO_BUILD_JOBS=caller-jobs
export TEMP=caller-temp TMP=caller-tmp TMPDIR=caller-tmpdir
cargo() {
    [[ "$PWD" == "$root" && "$CARGO_HOME" == "$root/.cache/cargo" && "$CARGO_TARGET_DIR" == "$root/target" && "$CARGO_BUILD_JOBS" == 4 ]] || return 91
    [[ "$TEMP" == "$root/.tmp" && "$TMP" == "$TEMP" && "$TMPDIR" == "$TEMP" && -d "$TEMP" ]] || return 92
    [[ "$fixture_failure" == 0 ]] || return 23
    mkdir -p "$CARGO_TARGET_DIR/release"
    printf fixture > "$CARGO_TARGET_DIR/release/agensi-mcp"
}
export -f cargo
for fixture_failure in 0 1; do
    export fixture_failure
    source "$root/scripts/build.sh" --debug
    result=$?
    if [[ "$fixture_failure" == 0 ]]; then [[ "$result" == 0 ]] || exit 93; else [[ "$result" == 23 ]] || exit 94; fi
    [[ "$PWD" == "$foreign" && "$CARGO_HOME" == caller-home && "$CARGO_TARGET_DIR" == caller-target && "$CARGO_BUILD_JOBS" == caller-jobs && "$TEMP" == caller-temp && "$TMP" == caller-tmp && "$TMPDIR" == caller-tmpdir ]] || exit 95
done
printf 'Bash child containment and sourced caller restoration passed.\n'
"#).expect("Bash fixture");
    let command = [
        OsString::from("bash"),
        script.into_os_string(),
        fixture.root.clone().into_os_string(),
        fixture.foreign.clone().into_os_string(),
    ];
    assert_script_success(runner_fixture::capture_fixture_with_input(
        &fixture.bootstrap,
        &fixture.foreign,
        &command,
        b"",
        &runner_fixture::fixture_environment(&fixture.root),
        Duration::from_secs(5),
    ));
}

#[test]
fn packaging_publishes_only_verified_owned_outputs_and_preserves_existing_files() {
    let fixture = ScriptInstallation::new();
    let binary = fixture
        .root
        .join(if cfg!(windows) { "input.exe" } else { "input" });
    // Transaction oracles need native startup/extraction, not repeated compression of the full CLI.
    // Real CLI behavior and cryptographic packaging remain separate release-CI gates.
    let source = fixture.root.join("fixture.rs");
    fs::write(
        &source,
        r#"fn main() {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["status"] {
        std::process::exit(2);
    }
    println!("{{\"mode\":\"disconnected\",\"profiles\":[]}}");
}
"#,
    )
    .expect("UTF8 native transaction fixture");
    let command = [
        OsString::from("rustc"),
        "--edition=2024".into(),
        "-Copt-level=z".into(),
        "-Cstrip=symbols".into(),
        source.into_os_string(),
        "-o".into(),
        binary.clone().into_os_string(),
    ];
    assert_script_success(runner_fixture::capture_fixture_with_input(
        &fixture.bootstrap,
        &fixture.foreign,
        &command,
        b"",
        &runner_fixture::fixture_environment(&fixture.root),
        Duration::from_secs(5),
    ));
    assert_script_success(fixture.powershell(r#"
param($Root, $Foreign)
$ErrorActionPreference = 'Stop'
$utf8 = [Text.UTF8Encoding]::new($false)
$platform = if ($IsWindows) { 'windows-x64' } elseif ($IsMacOS) { if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') { 'macos-arm64' } else { 'macos-x64' } } else { 'linux-x64' }
$extension = if ($IsWindows) { '.zip' } else { '.tar.gz' }
$binary = Join-Path $Root $(if ($IsWindows) { 'input.exe' } else { 'input' })
[IO.File]::WriteAllText((Join-Path $Root 'Cargo.toml'), "[package]`nversion = `"2.1.6`"", $utf8)
# This fixture isolates publication transactions. Real cryptographic/native controls run separately in CI.
$global:FixtureVerifyCalls = 0
[IO.File]::WriteAllText((Join-Path $Root 'scripts/verify-release.ps1'), '$global:FixtureVerifyCalls++; if (-not [IO.File]::Exists($Path+".sig")) { throw "Missing fixture signature." }', $utf8)
[IO.File]::WriteAllText((Join-Path $Root 'scripts/verify-release.ps1'), 'param($Path,$Platform,$SigningDirectory,$SourceCommit,$Version)' + "`n" + [IO.File]::ReadAllText((Join-Path $Root 'scripts/verify-release.ps1'),$utf8), $utf8)
[IO.File]::WriteAllText((Join-Path $Root 'scripts/sign-release.ps1'), 'param($Path) [IO.File]::WriteAllText(($Path+".sig"),"fixture-signature",[Text.UTF8Encoding]::new($false))', $utf8)
[IO.File]::WriteAllText(($binary+'.sig'), 'fixture-signature', $utf8)
[IO.File]::WriteAllText((Join-Path $Root 'run.ps1'), '# fixture root', $utf8)
& git -C $Root init --quiet
& git -C $Root add Cargo.toml run.ps1
& git -C $Root -c core.hooksPath=hooks-disabled -c commit.gpgsign=false -c user.name=Fixture -c user.email=fixture@example.invalid commit --quiet -m fixture
if ($LASTEXITCODE -ne 0) { throw 'Fixture source commit failed.' }
$archive = Join-Path $Root ("dist/agensi-mcp-$platform$extension")
$checksum = $archive + '.sha256'
$failed = $false
try { & (Join-Path $Root 'scripts/package-release.ps1') -Platform $platform -BinaryPath $binary } catch { $failed = $true }
if (-not $failed -or (Test-Path -LiteralPath $archive) -or (Test-Path -LiteralPath $checksum)) { throw 'Failed extraction verification published an output.' }
[IO.Directory]::CreateDirectory((Join-Path $Root '.claude-plugin')) | Out-Null
[IO.File]::WriteAllText((Join-Path $Root '.claude-plugin/plugin.json'), '{"name":"fixture"}', $utf8)
& git -C $Root add .claude-plugin/plugin.json
& git -C $Root -c core.hooksPath=hooks-disabled -c commit.gpgsign=false -c user.name=Fixture -c user.email=fixture@example.invalid commit --quiet -m manifest
foreach ($preexisting in @($archive,$checksum,($archive+'.sig'),($checksum+'.sig'))) {
    [IO.File]::WriteAllText($preexisting, 'preexisting-sentinel', $utf8)
    $failed = $false
    try { & (Join-Path $Root 'scripts/package-release.ps1') -Platform $platform -BinaryPath $binary } catch { $failed = $true }
    if (-not $failed -or [IO.File]::ReadAllText($preexisting,$utf8) -cne 'preexisting-sentinel') { throw 'Preexisting output was overwritten or removed.' }
    [IO.File]::Delete($preexisting)
}
& (Join-Path $Root 'scripts/package-release.ps1') -Platform $platform -BinaryPath $binary
$expected = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ([IO.File]::ReadAllText($checksum,$utf8) -cne ($expected+'  '+[IO.Path]::GetFileName($archive)+"`n")) { throw 'Published checksum does not match verified archive.' }
if ($global:FixtureVerifyCalls -lt 3 -or -not [IO.File]::Exists($archive+'.sig') -or -not [IO.File]::Exists($checksum+'.sig')) { throw 'Signature verification or signature publication bypassed.' }
[IO.File]::Delete($archive); [IO.File]::Delete($checksum); [IO.File]::Delete($archive+'.sig'); [IO.File]::Delete($checksum+'.sig')
$global:FixtureArchive = $archive
$global:FixtureChecksum = $checksum
$global:ReplacePublished = $false
function global:Write-Output {
    param([Parameter(ValueFromPipeline)]$InputObject)
    if ([string]$InputObject -like 'Packaged and startup-verified:*') {
        if ($global:ReplacePublished) {
            [IO.File]::WriteAllText($global:FixtureArchive, 'external-archive', [Text.UTF8Encoding]::new($false))
            [IO.File]::WriteAllText($global:FixtureChecksum, 'external-checksum', [Text.UTF8Encoding]::new($false))
        }
        throw 'Controlled post-publication failure.'
    }
    $InputObject
}
foreach ($replace in @($false,$true)) {
    $global:ReplacePublished = $replace
    $failed = $false
    try { & (Join-Path $Root 'scripts/package-release.ps1') -Platform $platform -BinaryPath $binary } catch { $failed = $true }
    if (-not $failed) { throw 'Controlled publication failure reported success.' }
    if ($replace) {
        if ([IO.File]::ReadAllText($archive,$utf8) -cne 'external-archive' -or [IO.File]::ReadAllText($checksum,$utf8) -cne 'external-checksum') { throw 'Concurrent output replacements were removed.' }
    } elseif (@($archive,$checksum,($archive+'.sig'),($checksum+'.sig') | Where-Object { Test-Path -LiteralPath $_ }).Count) { throw 'Handled failure left owned final outputs.' }
}
if (@(Get-ChildItem -LiteralPath (Join-Path $Root 'dist') -Directory).Count -ne 0 -or $PWD.Path -cne $Foreign) { throw 'Owned staging or caller CWD leaked.' }
'Packaging failure/retry/hash, preexisting outputs and conditional compensation passed.'
"#));
}

#[test]
fn evidence_collector_uses_its_own_checkout_and_restores_foreign_cwd() {
    let fixture = ScriptInstallation::new();
    assert_script_success(fixture.powershell(r#"
param($Root, $Foreign)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$PSNativeCommandUseErrorActionPreference = $false
$utf8 = [Text.UTF8Encoding]::new($false)
foreach ($directory in @($Root,$Foreign)) {
    & git -C $directory init --quiet
    if ($LASTEXITCODE -ne 0) { throw 'Fixture Git init failed.' }
    [IO.File]::WriteAllText((Join-Path $directory 'marker.txt'), $(if ($directory -eq $Root) { 'target-é' } else { 'foreign-é' }), $utf8)
    & git -C $directory add marker.txt
    & git -C $directory -c core.hooksPath=hooks-disabled -c commit.gpgsign=false -c user.name=Fixture -c user.email=fixture@example.invalid commit --quiet -m fixture
    if ($LASTEXITCODE -ne 0) { throw 'Fixture commit failed.' }
}
$expected = (& git -C $Root rev-parse HEAD).Trim()
$global:FormatCwd = $null
$global:FormatFail = $false
function global:cargo {
    $global:FormatCwd = $PWD.Path
    $global:LASTEXITCODE = if ($global:FormatFail) { 19 } else { 0 }
}
& (Join-Path $Root 'scripts/collect-ci-evidence.ps1')
if ($PWD.Path -cne $Foreign -or $global:FormatCwd -cne $Root) { throw 'Collector changed caller CWD or formatted another checkout.' }
$target = Join-Path $Root '.tmp/ci-review'
if ([IO.File]::ReadAllText((Join-Path $target 'source-commit.txt'), $utf8).Trim() -cne $expected) { throw 'Wrong source SHA collected.' }
Push-Location $target
try { $marker = & tar -xOf source.tar source/marker.txt; if ($LASTEXITCODE -ne 0) { throw 'Fixture source extraction failed.' } } finally { Pop-Location }
if (($marker -join "`n") -cne 'target-é') { throw 'Foreign source entered archive.' }
if ((Get-Item -LiteralPath (Join-Path $target 'format.patch')).Length -ne 0 -or [IO.File]::ReadAllText((Join-Path $Foreign 'marker.txt'),$utf8) -cne 'foreign-é') { throw 'Foreign diff/source was changed or collected.' }
$global:FormatFail = $true
& (Join-Path $Root 'scripts/collect-ci-evidence.ps1')
if ($LASTEXITCODE -ne 1 -or $PWD.Path -cne $Foreign) { throw 'Cargo failure did not preserve exit/CWD.' }
$global:FormatFail = $false
function global:git { $global:LASTEXITCODE = 17 }
$failed = $false
try { & (Join-Path $Root 'scripts/collect-ci-evidence.ps1') } catch { $failed = $true }
if (-not $failed -or $PWD.Path -cne $Foreign) { throw 'Git failure did not restore CWD.' }
'Collector root, UTF8, foreign sentinel and failure controls passed.'
"#));
}
