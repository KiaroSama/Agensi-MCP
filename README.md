# Agensi MCP

<div align="center">

[![CI](https://github.com/KiaroSama/Agensi-MCP/actions/workflows/ci.yml/badge.svg)](https://github.com/KiaroSama/Agensi-MCP/actions/workflows/ci.yml)
[![GPLv3](https://img.shields.io/badge/License-GPLv3-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.99%2B-dea584)](Cargo.toml)
[![PowerShell](https://img.shields.io/badge/PowerShell-7-5391fe)](run.ps1)
[![Windows](https://img.shields.io/badge/Platform-Windows-0078d4)](#start)
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

The `2.1.1` patch packages the integrated runtime audit repairs: routed OAuth resource ownership,
strict callback authority/framing, bounded state reads and coordinated client-registration recovery.
For an existing source checkout, update its public scripts/source first, verify the matching release,
and replace only `bin/agensi-mcp.exe` on Windows (or `bin/agensi-mcp` on Unix). Keep a recoverable
previous executable; do not extract the archive over saved user data. Reconnect clients after the
upgrade. A successful download or source test does not prove your saved sign-in or actual client
registration; verify those separately with `probe` and the intended client.

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
The artifact ZIP contains the platform archive plus checksum; extract the outer artifact first, then
the platform archive. Releases provide durable public downloads without an Actions account.

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

These builds are currently unsigned. Windows or macOS may require an explicit security approval;
check the download source and checksum, and follow local policy rather than disabling protections.

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
by this application. It adds no timezone metadata/header; UTC log timestamps are not sent upstream.
Browser login/checkout routing and browser timezone remain outside this bridge.

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

## Inline skill files

After specific approved free/owned one-time install, save returned path/content array or `files` object
as local JSON, then:

```bash
./bin/agensi-mcp.exe save-skill example ./returned-files.json
```

Exact UTF-8, root SKILL.md, max500files/20MiB; traversal, ADS/device/linked ancestors, case duplicates
and existing packages refused. Staging/atomic rename preserves originals. No script execution or global
activation. Cloud-only subscriptions stay cloud-only; cost/data/trial/install/creator actions need
separate scope/approval. Submission preview itself transfers files externally.

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
and bounded CI checks use unique `logs/register_YYYY-MM-DD_HH-mm-ss_UTC_<uuid>.log` files. Entries:
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

Actual competitor sales/revenue are not exposed by the current20-tool upstream MCP. This bridge
cannot manufacture that data. Installs and ratings remain public uptake signals, not transactions;
own Creator Studio analytics require separately authorized account access.

## Optional source build (developers only)

Ready-made downloads are the normal installation path. To modify the Rust source, clone this repository
and use an existing Rust1.99+ toolchain with native compiler prerequisites. Building is optional:

```bash
pwsh -NoProfile -File scripts/build.ps1
```

On macOS/Linux:

```bash
bash scripts/build.sh
```

Build scripts keep Cargo downloads in `.cache/cargo` and outputs in `target/`. After preserving the
installed executable in `bin/`, those two generated folders can be removed to reclaim disk space.
Do not remove `.state/auth`, `config.local.json`, your private proxy list or operational logs.

## Tests and CI

```bash
cargo test --locked --all-targets -- --test-threads=2
```

Use project CARGO_HOME/target environment as in build script. Windows launcher test requires built
`bin/agensi-mcp.exe`; Linux tests cover portable core, not Windows GUI/client integration.

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
Dependency Graph capability. CI tests run through `scripts/run-check.py` with explicit wall/idle
bounds, captured UTF-8 output, exit-code propagation and owned-tree termination. Cases have local
deadlines; jobs have outer budgets and constrained compile/test concurrency. These synthetic checks
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
