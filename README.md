# Agensi MCP

<div align="center">

[![CI](https://github.com/KiaroSama/Agensi-MCP/actions/workflows/ci.yml/badge.svg)](https://github.com/KiaroSama/Agensi-MCP/actions/workflows/ci.yml)
[![GPLv3](https://img.shields.io/badge/License-GPLv3-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.99%2B-dea584)](Cargo.toml)
[![PowerShell](https://img.shields.io/badge/PowerShell-7-5391fe)](run.ps1)
[![Windows](https://img.shields.io/badge/Platform-Windows-0078d4)](#installation)
[![Linux core](https://img.shields.io/badge/Core-Linux-64748b)](.github/workflows/ci.yml)
[![MCP](https://img.shields.io/badge/Protocol-MCP-7c3aed)](https://modelcontextprotocol.io/)
[![Streamable HTTP](https://img.shields.io/badge/Transport-Streamable_HTTP-2563eb)](https://agensi.io/mcp)
[![rmcp](https://img.shields.io/badge/SDK-rmcp-64748b)](https://github.com/modelcontextprotocol/rust-sdk)
[![Reqwest](https://img.shields.io/badge/HTTP-reqwest-334155)](https://github.com/seanmonstar/reqwest)
[![SOCKS](https://img.shields.io/badge/Proxy-SOCKS-0f766e)](#routing)
[![Docs](https://img.shields.io/badge/Docs-README-0284c7)](#operator-guidance)
[![Support donations](https://img.shields.io/badge/Support-donations-d04a9a)](#donate)

</div>

Native Rust connection manager and stdio↔Streamable HTTP Agensi bridge, with named fail-closed
proxies, manual OAuth and original multi-client operating guidance. No Node runtime is invoked.
Agensi's server stays at https://mcp.agensi.io/mcp; this independent project is not an official Agensi
product, marketplace listing or account authorization.

## Installation

There are two independent installs: the **Rust MCP connection manager** and the **instruction plugin**.
Installing the plugin does not build/start the server, register an MCP connection or log you into Agensi.
The plugin alone needs no Rust/Node dependencies. Never copy private configuration, OAuth state,
operational logs or build caches into a plugin installation.

### 1. Download a ready-to-run build

Open [Releases](https://github.com/KiaroSama/Agensi-MCP/releases/latest). Download **one** archive for
your machine and its matching `.sha256` file. No Rust, Cargo, Node, Git or local compilation is needed.

| Machine | Download |
|---|---|
| Windows x64 | `agensi-mcp-windows-x64.zip` |
| Linux x64 (Ubuntu24.04/glibc-compatible) | `agensi-mcp-linux-x64.tar.gz` |
| macOS Apple Silicon | `agensi-mcp-macos-arm64.tar.gz` |
| macOS Intel | `agensi-mcp-macos-x64.tar.gz` |

Each archive contains the compiled executable in `bin/`, launchers, original plugin, GPL license and
corresponding source. `BUILD-INFO.txt` records its source commit. Extract into a **new folder** and keep
the directory layout; don't move only the executable. An upgrade should not overwrite existing proxy,
authentication or operational files. Old versions remain available as explicit rollback downloads.

The `2.1.5` patch normalizes recognized structured device timezone metadata to fixed UTC/zero
in both stdio and local HTTP, without changing OS/browser settings or domain scheduling inputs.
The `2.1.4` patch rejects all documented Windows COM/LPT superscript device aliases in
skill-file paths on every platform before staging; valid Unicode and near-miss names remain supported.
The `2.1.3` patch bounds developer-runner writes when a downstream data-pipe consumer stops
reading, preserving partial output, interruption and pipe state. It includes the `2.1.2`
Rust-only tooling and Windows bootstrap handle-inheritance repair. It retains the `2.1.1` runtime audit repairs:
routed OAuth resource ownership, strict callback authority/framing, bounded state reads and
coordinated client-registration recovery.
For an existing source checkout, update its public scripts/source first, verify the matching release,
and replace only `bin/agensi-mcp.exe` on Windows (or `bin/agensi-mcp` on Unix). Keep a recoverable
previous executable only until the matching release/source/hash, installed version/startup and preserved
configuration/authentication are verified; do not extract over saved user data. Reconnect clients after
the upgrade. Verify the intended connection with `probe` and separately in the actual client: a native
probe is not proof of client loading. After all upgrade postconditions succeed, recycle unlocked,
operation-owned obsolete executables and download/build/temp copies (use the Recycle Bin on Windows).
Preserve the current executable, user state, skills, real operational logs and required private recovery
evidence. Failed/partial upgrades retain their recovery copy; locked or ownership-ambiguous files stay
reported, never client-killed just for cleanup. Public verified releases remain explicit rollback sources.

Verify SHA256 before running. On Windows PowerShell, from your downloads folder:

```bash
pwsh -NoProfile -Command "Get-FileHash ./agensi-mcp-windows-x64.zip -Algorithm SHA256"
```

Compare the displayed hash with the matching downloaded `.sha256` file. On Linux:

```bash
sha256sum -c agensi-mcp-linux-x64.tar.gz.sha256
```

On macOS Apple Silicon (use the x64 filename on Intel):

```bash
shasum -a 256 -c agensi-mcp-macos-arm64.tar.gz.sha256
```

Alternatively, open the latest successful [CI run](https://github.com/KiaroSama/Agensi-MCP/actions/workflows/ci.yml)
and download its platform artifact while it is retained (30days, GitHub sign-in may be required).
Historical release-run artifact ZIPs contain the platform archive plus checksum; extract the outer
artifact first, then the platform archive. New empty-tag validation runs do not create downloadable
release assets. Existing Releases provide durable public downloads without an Actions account.

### 2. Run on Windows

Open the extracted folder. PowerShell7 is needed only for the menu; the native executable can run
commands directly without PowerShell. No development toolchain is needed.

```bash
pwsh -NoProfile -File run.ps1
```

Or inspect the native executable directly:

```bash
./bin/agensi-mcp.exe status
```

Existing published builds through `2.1.5` are unsigned; hashes do not establish publisher trust.
From `2.1.6`, the release contract requires publisher **KiaroSama**: certificate-backed Windows
Authenticode/macOS codesign plus RSA3072/SHA256 detached signatures over exact binaries, archives
and checksum files. This is an approved **private self-signed identity**, not public-CA trust,
Apple Developer ID/notarization or a trusted timestamp. OS warnings can remain; never disable OS
protections or install a root certificate just to silence them. A release is available only after
its actual four-platform signing and verification gates pass.

Verification commands, pinned-key trust, expiry and signing-tool prerequisites are in the
[private release-signing contract](docs/migration-contracts.md#private-release-signing-contract).

### 3. Run on macOS or Linux

Extract the archive (Apple Silicon example; choose your platform filename):

```bash
tar -xzf agensi-mcp-macos-arm64.tar.gz
```

Open its extracted directory; Unix archive permissions preserve the executable bit. Bash is needed
for `run.sh`, not for running `bin/agensi-mcp` directly. No PowerShell or compiler is required.

```bash
bash run.sh status
```

Import a private proxy list (example filename, not provided credentials):

```bash
bash run.sh import /absolute/path/to/my-private-proxies.txt
```

```bash
bash run.sh select "Example Proxy"
```

```bash
bash run.sh signin
```

Keep the terminal open during browser approval. Use `bash run.sh bridge` for stdio, or `bash run.sh http`
for the local HTTP listener. Client stdio configuration uses an absolute path to `bin/agensi-mcp`
and `args: ["bridge"]` on Unix, rather than the Windows `.exe` example below.

### 4. Connect and verify

Downloaded binaries create no Cargo build cache. Runtime state and cache behavior are explained in
[Cache and state paths](#cache-and-state-paths). Preserve the installed `bin/` executable, route
settings and saved session when clearing optional development artifacts.

The Windows menu provides these setup actions:

1. Import proxies with option1 (default local list beside launcher).
2. Select a named profile with option2. Initial mode is disconnected; option4 explicitly enables direct.
3. Use option7 and copy the complete real authorization URL into an external browser on this machine.
4. Keep listener open until approval; reconnect your MCP client. Option6 registers the native executable.

Main menu shows only connection/selection/count; saved profiles appear in selection submenu. `exit`
quits. A registration conflict is refused; review existing settings rather than silently replacing them.
Registration honors explicit targets and `CLAUDE_CONFIG_DIR` for Code's `.claude.json`; it validates
both UTF-8 object-shaped documents before mutation (32 MiB per client config, distinct from the
native 1 MiB state limit). Same-directory private staging, cooperative locks and exact-byte read-back
prevent silent partial success. A later failure restores only unchanged operation-written targets;
concurrent edits remain untouched and unresolved original bytes stay in protected recovery files.
This is not crash/power-loss atomicity; empty lock files are coordination metadata, not cleanup trash.
To explicitly upgrade only this project's old Node/native Agensi entry (other servers are preserved):

```bash
pwsh -NoProfile -File scripts/register.ps1 -UpgradeProjectEntry
```

An entry belonging to a different project is refused even with this switch.

## Desktop entry

After build, merge this example into `mcpServers`, replacing the example root with your actual folder.
Other server entries must remain unchanged.

```json
{
  "mcpServers": {
    "agensi": {
      "command": "<absolute-project-root>/bin/agensi-mcp.exe",
      "args": ["bridge"],
      "env": {
        "TEMP": "<absolute-project-root>/.tmp",
        "TMP": "<absolute-project-root>/.tmp",
        "TMPDIR": "<absolute-project-root>/.tmp"
      }
    }
  }
}
```

On Windows JSON paths can use `/` or escaped `\\`. The native binary resolves its installation root
from its own ancestors; caller CWD and spaces do not change route/auth/log storage.

## Routing

HTTP/HTTPS/SOCKS4/SOCKS4a/SOCKS5/SOCKS5h. Ordinary4/5 use local destination DNS;4a/5h use proxy-side
DNS in stored profile names, but the application now normalizes both to SOCKS4a/SOCKS5h at runtime:
destination DNS always occurs through the selected proxy. Stored host, port, credentials and profile
selection remain unchanged. Proxy-host DNS can still be local. TLS certificates remain verified.

Proxy-side resolution prevents local destination DNS, but the provider may use a resolver in another
country or an anycast address. Matching proxy egress country and resolver country cannot be guaranteed
by this application. It does not collect the OS timezone or inject absent timezone fields; UTC log
timestamps are local. Browser login/checkout routing and browser timezone remain outside this bridge.

### Device timezone privacy

An always-on outbound policy applies to stdio and local HTTP. Existing recognized structured device
zone fields become `UTC` (zero offset all year); numeric offsets become `0`, string offsets become
`"0"`. It covers requests, notifications and structured client replies before upstream transmission:

- Device-qualified keys: `deviceTimezone`, `systemTimezone`, `clientTimezone`, `browserTimezone`,
  `localTimezone`, and the same prefixes with `TimezoneOffset`, `TimezoneOffsetMinutes`, `UtcOffset`,
  `UtcOffsetMinutes` or `UtcOffsetSeconds`; ASCII case and snake/kebab/dot variants are accepted.
- Inside `_meta` or explicit `device`, `system`, `browser`, `clientInfo`, `clientContext`,
  `deviceInfo`, `deviceContext`, `systemInfo`, `systemContext`, `browserInfo`, `browserContext` and
  `userLocation` objects: `timezone`, `tz`, `timezoneId`, `ianaTimezone` and the bare offset keys above.
  Namespaced objects such as `_meta["openai/userLocation"].timezone` are supported.
- Zone objects normalize existing `name`/`id`/`zone` and offset members. Unsupported recognized zone
  value shapes become `UTC` rather than retaining the original value. Device-qualified
  `mcp-param-*` routing hints also become UTC/0; ordinary `timezone`/`x-timezone` headers stay excluded.

Unchanged HTTP messages keep their original bytes; malformed JSON fails before authorization/egress.
No OS/browser settings, saved credentials, location coordinates or absent fields are changed. A bare
business `arguments.timezone` or `mcp-param-timezone` remains unchanged. Content/text/prompt/code/files,
schemas/defaults/enums/examples, error data and opaque `requestState` are not inspected. Generic
elicitation answers cannot reliably be distinguished from scheduling answers and are not rewritten.
This is a documented structured-metadata policy, **not** universal free-text/encoded-data filtering,
browser fingerprint protection, or prevention of timezone/location inference from IP/account history.
MCP itself does not define a required timezone field; vendor hints are not proof Agensi requests them.

No direct/alternate fallback or environment NO_PROXY bypass. Invalid/unreachable selected proxy blocks
traffic. A route change invalidates old relay within the200ms watcher; reconnect clients. Unknown
charged/upload outcomes are reconciled before retrying, never automatically replayed.

Reqwest handles standard routes. A narrow native authenticated loopback adapter preserves SOCKS4
user IDs that reqwest's upstream connector otherwise ignores. It always tunnels to the selected proxy,
never the destination directly; owns bounded sessions and closes when dropped. SOCKS4 has no password
or IPv6 destination. Browser OAuth/checkout routing is separate; this is not an OS-wide firewall.

## Local HTTP endpoint

Stdio remains available. To serve local MCP HTTP at the configured port (default1594):

```bash
./bin/agensi-mcp.exe http
```

Endpoint: `http://127.0.0.1:1594/mcp`. Change the saved port, then restart the listener:

```bash
./bin/agensi-mcp.exe http-port 2594
```

The `local_http_port` setting accepts1..65535. This is not the OAuth callback, upstream HTTPS443 or
selected proxy port. Host/Origin must match the exact local listener; no external bind, arbitrary
forwarding destination, wildcard CORS or redirects. The owner selected no local token: local processes
are trusted and can use the saved Agensi session. Never expose or forward this port publicly.

## Cache and state paths

The executable derives the project root from its own installed location. Runtime files are explicitly
project-local; Desktop `env` values above additionally direct generic dependency temporary files into
project `.tmp`. Create that directory before launch (the registration script creates it automatically). Saved OAuth session: `.state/auth/rust-session.json`; refresh lock
alongside it. Proxy/settings: `config.local.json`; logs: `logs/`. No native runtime download cache is
needed to run a built binary. Cargo's build/download cache is `.cache/cargo` only during builds, with
build outputs in `target`; build scripts set those paths explicitly. These paths are independent of
Claude Desktop's own application cache, which this project does not control.

## Proxy import and management

UTF-8 six-field rows with final display name; example values only:

```text
socks5://127.0.0.1:1080:example-user:example-password:Example Proxy
```

Import is additive/atomic, skips identical duplicates, rejects conflicting names without altering route.
Malformed rows report only sanitized line numbers. Limits100 saved profiles/1MiB list; field values
cannot contain colons. Proxy passwords never belong in public source or chat.

```bash
./bin/agensi-mcp.exe status
```

In redacted status, `authenticated: true` means a configured username or password is present for a
supported proxy, including password-only HTTP credentials and SOCKS4 USERID. It does not prove a
successful proxy handshake or service sign-in. Status never prints credentials or full proxy URLs.

```bash
./bin/agensi-mcp.exe select "Example Proxy"
```

## Manual OAuth

```bash
./bin/agensi-mcp.exe signin
```

Current official SDK handles discovery/scopes/PKCE/state/issuer/token refresh. Human opens the genuine
full URL; no automatic browser. Five-minute callback budget and stable local callback identity.
Callback requests require exactly one `Host: 127.0.0.1:<callback-port>` in both HTTP/1.0 and HTTP/1.1,
GET on the fixed callback path, complete bounded headers and no body framing. Malformed/idle peers
are rejected without ending the remaining sign-in window. Host hardening does not replace state,
issuer or PKCE checks and is not proof that arbitrary local programs are authenticated. Safe
issuer/scope/expiry-compatible public-client legacy sessions can be imported without extending receipt
time; original data is preserved. Incompatible caches require human renewed login, not weakened checks.
Stored credentials are project-local, not encrypted vault storage; restrict directory access.

Public connectivity, account read, creator/payout readiness and submitted/approved/live listing are
separate evidence layers. Read-only `probe` lists tools and checks an account-scoped response; it does
not buy, upload or verify creator eligibility. Never paste tokens/codes/callback URLs in chat.

## Agent-only Agensi skills

Agensi's October12,2026 model loads the latest approved skill through your agent for each use;
buyers no longer download or install acquired instructions. Search, inspect the offer, approve any
purchase, then fetch `get_skill` for the requested use and follow the content in-session. Do not save
acquired instructions to local skill directories, caches, logs or exports. Existing local files are
not deleted by this update. This is not a guarantee that your client/provider never persists chat.

Credits now represent dollars (1 credit = $1), one-off top-ups start at $5 and expire after12months.
One-time skills under$5 require credits; those at$5 or above support card or credit. The agent asks
before buying. `buy_credits` returns a checkout link for a user-chosen amount; it never charges itself.
`review_skill` posts only the user's explicit rating and words. The old free-run allowance and
Starter/Pro credit plans are retired; individual monthly subscription skills remain a separate
card-billed/server-side Terms exception. One connected agent at a time: another sign-in disconnects
the previous agent. Some live tool descriptions still advertise old trial/file-update behavior;
that metadata does not override current licensing or approval requirements.

Current [operator workflow](skills/agensi-operator/references/tools-and-workflows.md) and
[policy guidance](skills/agensi-operator/references/policies.md) cover approval, refunds, rights,
creator70%share/$1minimum, subscription exceptions and remaining provider inconsistencies.
The original local `agensi-operator` plugin is our guidance, not acquired marketplace content.

## Original local skill files

The manual helper accepts **original or independently authorized content**,
**not for acquired Agensi instructions**. For a permitted local path/content array or `files` object:

```bash
./bin/agensi-mcp.exe save-skill example ./returned-files.json
```

Exact UTF-8, root SKILL.md, max500files/20MiB; traversal, ADS/device/linked ancestors, case duplicates
and existing packages refused. Staging/atomic rename preserves originals. No script execution or global
activation. The helper does not infer a licence from a JSON input; the caller verifies rights first.
Marketplace content uses the agent-only flow above. Cost/data/creator actions need separate approval;
submission preview itself transfers files externally.

## Operator guidance

The project root is the plugin root, matching the Revayat skill layout: `.claude-plugin/`,
`.codex-plugin/`, `.cursor-plugin/` and [skills/agensi-operator](skills/agensi-operator/SKILL.md)
are siblings. Root `plugin.json` is the minimal native Antigravity format; the other clients use
same-name compatibility manifests. No duplicate nested package is needed. No plugin MCP/autostart,
hooks or tool preapproval. File presence is not native loading evidence.

Claude Code installation:

```text
/plugin install agensi-operator --marketplace KiaroSama/Agensi-MCP
```

Session-only local loading:

```bash
claude --plugin-dir .
```

Codex standalone discovery uses the original skill copied to the target project's `.agents/skills/agensi-operator`;
Cursor uses `.cursor/skills/agensi-operator` or one supported compatible location. Antigravity native
project plugins use `.agents/plugins/agensi-operator`, or standalone `.agents/skills/agensi-operator`.
Do not create duplicate discovery copies. Native global installation is a separate approved action. Full per-client steps are below.

### Install the Claude Code plugin

In an interactive Claude Code session:

```text
/plugin install agensi-operator --marketplace KiaroSama/Agensi-MCP
```

Start a new session and invoke `/agensi-operator:agensi-operator`. For a local session-only test, run
`claude --plugin-dir .` from the cloned repository instead. Validate packaging without installation:

```bash
claude plugin validate .claude-plugin/plugin.json
```

### Install the Codex plugin

The root `.codex-plugin/plugin.json` is the native compatibility manifest. To install through a local
repo marketplace, first copy only these original plugin files into a target project's
`plugins/agensi-operator/`: `.codex-plugin/plugin.json`, `skills/agensi-operator/`, `LICENSE` and
`plugin.json`. Preserve existing target files; do not recursively copy this whole MCP installation.

Add this entry to the target project's `.agents/plugins/marketplace.json` (merge `plugins`, do not
replace an existing catalog):

```json
{
  "name": "agensi-local",
  "plugins": [
    {
      "name": "agensi-operator",
      "source": { "source": "local", "path": "./plugins/agensi-operator" },
      "policy": { "installation": "AVAILABLE", "authentication": "ON_INSTALL" },
      "category": "Productivity"
    }
  ]
}
```

The source path resolves from the **target project root**, not from `.agents/plugins/`. Restart the
supported Codex desktop surface and install from this local marketplace. Codex CLI provides `/plugins`
for configured catalogs; availability depends on the client/version. Codex IDE does not support native
plugins: copy only `skills/agensi-operator` to target `.agents/skills/agensi-operator` as a standalone
skill instead. Verify discovery in a new session before claiming installation.

### Install the Cursor plugin

1. Create `~/.cursor/plugins/local/agensi-operator/` on the machine running Cursor.
2. Copy only `.cursor-plugin/plugin.json`, `skills/agensi-operator/` and `LICENSE` into it, keeping their
   relative structure. Do not copy the Antigravity root `plugin.json` into this Cursor-native package.
3. Restart Cursor or use **Developer: Reload Window**.
4. Open **Customize** and confirm `agensi-operator` and its skill are present; invoke `/agensi-operator`.

On Teams/Enterprise, local plugin imports can be disabled by the administrator. A marketplace plugin
with the same name takes precedence. A symlink to a repository outside the local-plugin directory is
not a supported substitute. Standalone workspace alternative: target `.cursor/skills/agensi-operator`.
This project has not been submitted to Cursor's public marketplace.

### Install the Antigravity plugin

From the cloned repository, on a machine with the Antigravity CLI:

```bash
agy plugin install .
```

```bash
agy plugin list
```

For workspace-only manual installation, place only `plugin.json`, `skills/agensi-operator/` and
`LICENSE` under the target workspace `.agents/plugins/agensi-operator/`. Antigravity2.0/IDE uses
that workspace path; global manual installation uses `~/.gemini/config/plugins/`. CLI-managed assets
use its own documented configuration location. Do not mix these scopes or overwrite another installed
plugin silently. Open a new supported session and verify the skill appears.

### Installation boundaries

The shared original instruction payload is the same for all four clients, but native manifests and
installation methods differ. Root `plugin.json` is Antigravity-native, not a claimed universal Agent
Plugins manifest. No installer here executes copied scripts or automatically configures service access.
Claude manifest validation has been run; actual four-client installation/loading remains unverified.
Sources checked2026-10-11: [Claude plugins](https://code.claude.com/docs/en/plugins),
[Codex packaging](https://developers.openai.com/plugins/build/plugins),
[Cursor local plugins](https://cursor.com/docs/plugins), [Antigravity plugins](https://antigravity.google/docs/plugins).

[Paired-output experiment](docs/operator-evaluation.md): competent baseline also meets core criteria;
no proven statistical effectiveness lift or real payment validation. Installs do not establish sales,
conversion, revenue or causality. No private/acquired marketplace instructions redistributed.

## Logging

New perrun UTF-8 `logs/agensi-native_YYYY-MM-DD_HH-mm-ss_UTC_<pid>_<uuid>.log`. Registration
uses unique `logs/register_YYYY-MM-DD_HH-mm-ss_UTC_<uuid>.log` files; bounded CI checks use
`logs/check_YYYY-MM-DD_HH-mm-ss_UTC_<uuid>.log`. The native release identity guard uses
`logs/release_YYYY-MM-DD_HH-mm-ss_UTC_<pid>_<uuid>.log`; Windows menu checks use the same naming
form with `menu-tests`. The independent audit uses `audit-blackbox` logs inside its owned temporary
invocation; successful scratch is removed, failed diagnostics are retained for review. The check runner refuses linked log paths,
owns its exclusive UTF-8 log stream, and enables safe DEBUG status with `--debug`. Entries:
`[YYYY-MM-DD HH:mm:ss UTC] [LEVEL] [COMPONENT] Message`. Safe startup/failure/shutdown metadata only;
no protocol bodies, proxy credentials/tokens/auth URLs. File-init failure uses stderr. Vendor debug
logging stays disabled because third-party protocol logs may expose credential bytes. Retention manual;
share reviewed sanitized excerpts only. Preserve real operational logs during cleanup.

## Local diagnostics and cleanup

These are native **local CLI commands**, not additional Agensi account privileges or server tools.

```bash
./bin/agensi-mcp.exe doctor
```

Reports build version, selected route, local HTTP port and sanitized session age/expiry metadata.
It never prints tokens, client secrets or a full proxy URL. Token presence/expiry is not proof that
refresh or live account access will succeed.

```bash
./bin/agensi-mcp.exe network-check
```

Uses the same explicitly selected proxy and proxy-side destination DNS as the bridge. Bounded calls
to ipinfo.io, ipify.org and bash.ws inspect public egress and fresh authoritative DNS observations;
no Agensi bearer token is sent. Resolver geography can change. An unavailable/empty DNS response is
reported unknown, not proof of no leakage. No direct fallback or automatic profile switching.

```bash
./bin/agensi-mcp.exe cleanup
```

Preview only. To apply the exact reported eligible legacy removals:

```bash
./bin/agensi-mcp.exe cleanup --apply
```

Cleanup requires an idle application runtime and rejects linked/reparse paths. Only legacy
`node_modules` and this project's known old-binary backup `.ai/ARCHIVE/pre-ci-binary` are eligible.
Protected state, proxy configuration, skills/plugins, operational logs and installed `bin` remain.
Uncoordinated build caches (`target`, `.cache`), distribution downloads and shared `.tmp` contents are
reported skipped rather than guessed safe to delete while another build/client may be using them.

Operation-owned staging/temp files already use Rust RAII or script `finally` cleanup on normal and
handled-error paths. Abrupt OS/process termination can leave remnants; a shared temp root is not an
ownership marker, so automatic blanket deletion is intentionally absent. These commands also appear
as Windows menu10–13; use `bash run.sh <command>` on Unix.

Actual competitor sales/revenue are not exposed by the upstream MCP. This bridge
cannot manufacture that data. Installs and ratings remain public uptake signals, not transactions;
own Creator Studio analytics require separately authorized account access.

## Optional source build (developers only)

Ready-made downloads are the normal installation path. To modify the Rust source, clone this repository
and use an existing Rust1.99+ toolchain with native compiler prerequisites. Internal debug builds do
not replace `bin/` and are not completed distributable builds:

```bash
pwsh -NoProfile -File scripts/build.ps1 -Debug
```

On macOS/Linux:

```bash
bash scripts/build.sh --debug
```

Completed source builds require approved signing inputs and PowerShell7.4+; see the
[private release-signing contract](docs/migration-contracts.md#private-release-signing-contract).

Build scripts keep Cargo downloads in `.cache/cargo`, outputs in `target/` and child scratch in
`.tmp/`; they restore caller environment/location on success or failure. After the installed executable
and upgrade postconditions above are verified, recycle only unlocked operation-owned build/download
scratch to reclaim disk space. Keep active/shared data, current `bin/`, authentication, configuration,
skills, operational logs and unresolved-failure recovery evidence.

## Tests and CI

CI is the normal full-verification path; its [target mapping](.github/workflows/ci.yml) mechanically
checks every test target. For an authorized developer light check, create project-local `.tmp/`, set
`CARGO_HOME=.cache/cargo`, `CARGO_TARGET_DIR=target`, `CARGO_BUILD_JOBS=4` and absolute project-local
`TEMP`, `TMP`, `TMPDIR` as in the build scripts. Bootstrap once, then copy the runner before further
Cargo use (Windows cannot overwrite its running executable):

```bash
cargo +1.99.0 build --locked --bin check-run
```

```bash
cp target/debug/check-run .tmp/check-run
```

On Windows use PowerShell `Copy-Item target/debug/check-run.exe .tmp/check-run.exe` and `.exe` for
all runner paths below. Precompile targets under the compile budget, then select ordinary libtest targets
explicitly; `--all-targets` is for compilation here, not a universal test-harness command:

```bash
.tmp/check-run --wall 600 --idle 120 -- cargo +1.99.0 build --locked --all-targets
```

```bash
.tmp/check-run --wall 180 --idle 45 -- cargo +1.99.0 test --locked --lib --bin agensi-mcp --bin check-run --test management --test network --test auth --test tls --test protocol --test oauth_flow --test entry --test socks4_userid --test local_http --test settings --test diagnostics --test cleanup --test audit_regressions --test check_release --test operator -- --test-threads=2
```

The three custom harnesses do not accept libtest arguments. Run the owned-runner tier on all hosts,
menu on Windows only, and the independent audit on Linux only:

```bash
.tmp/check-run --wall 90 --idle 20 -- cargo +1.99.0 test --locked --test check_runner
```

```bash
.tmp/check-run.exe --wall 90 --idle 20 -- cargo +1.99.0 test --locked --test menu
```

```bash
.tmp/check-run --wall 120 --idle 20 -- cargo +1.99.0 test --locked --test audit_blackbox -- --source-root . --output .tmp/audit-results.json
```

Doc tests use `.tmp/check-run --wall 60 --idle 30 -- cargo +1.99.0 test --locked --doc`.
Output pressure uses `pwsh -NoProfile -File scripts/test-output-pressure.ps1` (its own bounded child
controls). Windows also runs both registration suites separately under `.tmp/check-run.exe --wall 90
--idle 20 -- pwsh -NoProfile -File`, with `scripts/test-registration.ps1` and
`scripts/test-registration-storage.ps1`. Windows menu tests require built `bin/agensi-mcp.exe`.
Before the Linux audit, create `bin/` and copy `target/debug/agensi-mcp` to `bin/agensi-mcp` with
executable permission; the independent harness deliberately runs that staged binary, not Cargo's test
executable. On Windows stage `target/debug/agensi-mcp.exe` into `bin/agensi-mcp.exe` for menu tests.
These are target-specific commands, not instructions to duplicate CI suites locally; Linux core tests
do not establish Windows GUI or real client integration. See [migration contracts](docs/migration-contracts.md).

CI starts only by an explicit `workflow_dispatch` on the completed source SHA; pushes, PRs and schedules
do not start validation. The one manual root calls same-commit RustSec and CodeQL workflows; release
publication waits for both security jobs and all four native platforms plus approved signing readiness.
An empty release tag runs internal debug/test validation only. A nonempty tag requires exact version,
approved public pins and encrypted leaf inputs; signing secrets are restricted to the verified owner,
repository and main or the single reviewed release branch. Publication remains main-only. Packaging
transaction fixtures use isolated test adapters; actual release controls independently reject wrong
signers/keys and tampered covered bytes before native/download publication. Four native jobs also run
the dedicated real-CLI HTTPS proxy/custom-CA phase with disposable trust/network setup and cleanup;
its results, not ordinary TLS unit tests, establish platform parity. The Linux evidence collector
intentionally formats only its own CI checkout to export a diagnostic patch after the independent
blocking formatting check; its output is not an extra PASS.

CI covers Windows2025 x64, Ubuntu24.04 x64, macOS15 Apple Silicon and macOS15 Intel: pinned Rust
toolchain, fmt, clippy warnings-as-errors, locked build, source-oracle management/callback parity,
real loopback proxy/DNS/auth handshakes, no-bypass/redirect, SOCKS4userid lifetime, SDK bidirectional
protocol/custom/cancel/backpressure/route change, OAuthPKCE callback/session, TLS certificate rejection,
native entry/menu and public-package checks. Version2.1 adds sanitized diagnostic projections,
captured-route consistency, cleanup preview/apply preservation, exclusive/shared runtime leases,
linked-path refusal and truthful partial-deletion checks. Release jobs additionally verify extracted
archives, executable bytes, startup and SHA256 checksums before publishing four platform downloads.
The Linux independent synthetic black-box suite executes all 98 required cases, including positive
SOCKS4 USERID sign-in, route revocation, malformed/fragmented callbacks and protected local state.
Windows executes both isolated registration suites; release preflight rejects a label that differs
from the package version. RustSec checks the full lockfile independently of the optional GitHub
Dependency Graph capability. Project tooling and test fixtures are Rust; no Python interpreter is
required. CI bootstraps the native `check-run` tool, then runs checks with explicit wall/idle
bounds, byte-preserving binary output, exit-code propagation and owned-tree termination. Maintained
source and status logs are UTF-8; arbitrary child output is relayed without decoding. Newline-free
chunks count as progress, buffering is bounded, and nonfinite/nonpositive deadlines are rejected
before launch. The runner has 18 regression methods plus four output-boundary unit tests. Four native-platform
pressure controls distinguish complete draining output from a stalled data-pipe consumer; a usable
status sink is required. See [Output pressure review](docs/output-pressure-review-2026-10-13.md).
Six native Windows menu methods cover EOF,
blank-line default import, exit aliases and machine-readable status. Cases have local deadlines;
jobs have outer budgets and constrained compile/test concurrency. See
[Native tooling commands](docs/migration-contracts.md#native-tooling-migration) for release guards and
platform-specific test targets. These synthetic checks
do not certify real account/browser or all four native client GUI integrations.
No tests purchase, publish or contact Agensi. Source parity and known limitations:
[Migration contracts](docs/migration-contracts.md). Positive custom-CA HTTPS-proxy parity and full
native four-client loading remain separate unverified scenarios; live read-only probe alone is not all
production behavior. Rust CodeQL and dependency metadata checks run alongside core CI.

## License

Copyright2026 Kiaro Sama. Original project code/guidance: [GNU GPLv3 only](LICENSE),
SPDX `GPL-3.0-only`. Dependencies and referenced sources retain their licenses.

## Donate

If this project helps you, donations are appreciated.

| Currency | Network | Address |
| --- | --- | --- |
| Bitcoin (BTC) | Bitcoin | `bc1qmth5m03pu5hujw5xw5jmywam3jj3sqwqupesdt` |
| USDT, BNB, USDC, etc. | BEP20 | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef` |
| USDT, TRX, USDC, etc. | TRC20 | `TWBA3xFTqgZAeAYMxqo85xWnzvty3DcAhw` |
| Ethereum (ETH) | ERC20 | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef` |
| TON | TON | `UQCN8Umo_OfOWqImZetQsrNStPcmLkMAKajFyiCOhso23NDb` |
| Litecoin (LTC) | LTC | `ltc1qntqnnrunadurnw4cshv3qgspywrueyyeyngwuy` |
| Solana (SOL) | Solana | `7B2wkczUjmkDhETwQuknBL8sUsbuV7nErxc317TmQuwR` |
| Polygon (POL) | Polygon | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef` |
