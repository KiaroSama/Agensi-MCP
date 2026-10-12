# Runtime audit — 2026-10-12

## Review contract and source identity

Follow the owner's actual Rules, hooks, research/documentation policy and prescribed Spec-Kit chain before integrating this work. The audit environment could read the published repository, but not the owner's private rule installation or execute its hooks. Those gates are not marked passed.

Implementation is in [PR #4](https://github.com/KiaroSama/Agensi-MCP/pull/4), against `33eaafc1a59d11d5cb3a49219df7f456e1bdf83e`. This audit does not merge or close PRs. The authorized reviewing agent must review all repository PRs under the owner's Rules, merge verified repairs, or manually integrate equivalent fixes before closing superseded proposals. An accepted unfixed defect is not DONE merely because its PR is closed.

**Identity gate:** the connector-created commits use the account's GitHub noreply identity because its commit API provides no author/committer override. Before integration, reconstruct the proposed commits with `Kiaro.Sama.Dev@gmail.com` in both raw author and committer fields. Inspect and repair older published history according to the owner's Rules and explicit history-rewrite authorization, preserving content and recording old/new mappings. A mailmap or message trailer is not a raw-history repair. This audit did not rewrite shared history.

## Evidence and validation boundary

The original official Linux release archive came from workflow run `38171690352`, artifact `11714610548`, source `1b492cc9dba13a075aa1a2836be3e1017c4502a9`. Its runtime source is identical to the audited main head; main subsequently changed two documentation files. Both archive and executable checksums were verified.

The first implemented head is `57ad566f29eca0704926e85983c205a9b5a96f43`. CI run [38181475480](https://github.com/KiaroSama/Agensi-MCP/actions/runs/38181475480) compiled its synthetic PR merge `d87fb545476e30b598ec17c7a7343e497b115b3c`. Native tests, Clippy, documentation tests and publication checks passed on Windows x64, Linux x64, macOS ARM64 and macOS x64. The isolated Windows registration tests also passed. Linux ran **75 native tests**. That first run remained red because the independent formatting gate correctly rejected unformatted code; its exact Rust 1.99 formatter patch is applied in this revision. The separate dependency action exposed an unavailable GitHub capability, addressed in A08 below. Check the latest PR SHA's actual CI before integration; earlier green individual steps are not a substitute.

Independent Linux black-box testing ran the same **51 cases** against both binaries in isolated project paths containing spaces, from a foreign working directory. The baseline passed 41 and failed 10; the implemented binary passed **51/51**. Its SHA256 is `5a7b046b9be0490b0cbb4c6c439a1cfaa725437f263d7bd9030dbe08539d792c`. Failures covered oversized settings, three FIFO paths, two skill filename variants, linked log writes, missing stderr fallback, and both SOCKS4 USERID schemes. These are ten failing assertions, not ten unrelated root causes.

No live Agensi credentials, account mutations, paid operations, real provider proxies or client GUI installation were used. Local Rust and PowerShell toolchains were unavailable; revised source compilation and PowerShell execution were performed by the existing multi-platform GitHub CI, and its Linux executable was downloaded and tested independently. Private hooks, owner-specific Windows ACLs/junction layouts, external-browser sign-in and actual Desktop/Code operation remain explicit owner-environment acceptance gates.

## A01 — High: OAuth drops the selected SOCKS4 USERID transport owner

**Root cause:** the sign-in call handed the authorization manager only `reqwest::Client`, dropping the sole `RoutedClient` owner of the local SOCKS4 USERID adapter. The request then used a dead loopback relay. No-USERID positive controls reached the selected fake proxy, while USERID variants did not.

**Repair:** `network::OAuthHttp` owns `RoutedClient`, and all authorization entrypoints transfer or clone that owner for the entire OAuth lifetime. The adapter is not leaked globally or replaced with direct networking.

**Closure tests:** `oauth_keeps_socks4_userid_adapter_alive_after_manager_takes_ownership` exercises the actual authorization manager for `socks4` and `socks4a`. Existing protocol-level adapter tests retain USERID/remote-DNS checks. Independent binary tests confirm both USERID variants now reach the deliberately rejecting loopback proxy; no fixture claims successful real sign-in.

## A02 — High: route construction and change detection can describe different settings

**Root cause:** reading config for a client and recapturing it later for OAuth/change detection could pair an old route with a new snapshot. HTTP forwarding could continue after settings changed during body/token acquisition. Some long operations lacked cancellation or a final stale-result check.

**Repair:** capture bounded config bytes once, parse those same bytes, construct the route, and propagate the same snapshot through `main`, `auth`, `bridge`, `local_http` and `diagnostics`. `network::watch_route` checks before starting, every 200 ms while pending, and before returning a completed result. Add explicit checks at HTTP and stdio forwarding boundaries; bound token acquisition and probe shutdown. A change, deletion, unreadable state or invalid file form fails closed. No route auto-selection, direct fallback or request replay is added.

**Closure tests:** manager refuses an intervening change before creating state; stale completed results are rejected; pending work is cancelled; an HTTP token fixture changes settings and proves a 503 with **zero upstream contact**. Existing route-change, capacity, SSE, server-request and no-replay cases remain active.

**Boundary:** polling cannot retract bytes already sent before a change, and is not atomic revocation against an adversarial filesystem writer. It is bounded cooperative cancellation plus forwarding checks.

## A03 — Medium: fragmented or unrelated callback traffic breaks sign-in

**Root cause:** one TCP read was treated as a complete HTTP request. TCP fragmentation could truncate the authorization query; an idle/malformed peer could abort the entire sign-in. Duplicate security-sensitive query parameters were ambiguous, and failure writing a browser reply could misreport already-persisted authorization as failure.

**Repair:** read through the header terminator with a 16 KiB ceiling and a five-second per-peer deadline; require GET, supported HTTP version, origin-form target and the exact callback path. Reject duplicate `state`, `code`, `iss` and `error` parameters. Return a bounded rejection and continue for irrelevant/malformed peers, but terminate a matching explicit authorization denial. Keep constant-time state comparison, SDK issuer verification, PKCE and the overall 300-second deadline. Browser replies are best-effort after the authorization result.

**Closure tests:** official SDK happy-path authorization saves a reusable issuer-bound session; a second fixture survives idle, invalid UTF-8, wrong-method and wrong-path peers before accepting a deliberately fragmented callback. Duplicate-parameter tests remain independent of the happy path.

## A04 — Medium: local state and operator input can block or escape intended paths

**Root cause:** native credentials and watcher snapshots were not consistently bounded; link/regular-file checks varied between readers, locks and writers. Opening a FIFO before validating its type could block indefinitely. A serialized config could exceed the reader's intended safety ceiling.

**Repair:** `storage::{check_file,read_optional,snapshot,matches}` provide shared cooperative link/reparse, regular-file and 1 MiB bounded reads. Check metadata before opening and verify the opened file. Apply them to settings, watchers, native/legacy credentials and credential/management locks. Cap serialized settings before atomic replacement. Reject nonregular proxy-list and inline-skill inputs before opening, retaining their existing size limits. Keep corrupt evidence, unknown config fields, legacy credentials and atomic replacement semantics intact.

**Closure tests:** exact-size and oversize reads, oversized-save preservation, native credential limits, symlinked credential directory/file/lock preservation, and bounded FIFO refusal. Independent binary cases verify oversized valid config plus `status`, `import` and `save-skill` FIFO rejection. Existing management/import, migration, lock coordination, cleanup and no-overwrite suites continue to pass.

**Boundary:** metadata checks do not provide race-free openat-style capability isolation against hostile component swaps or all hard-link attacks. Do not advertise that stronger guarantee.

## A05 — Medium: logs follow a linked directory and disappear silently on failure

**Root cause:** the original logger followed `logs/` symlinks outside the project; failed file setup/write operations silently discarded safe diagnostic records. Timestamp/PID-only names could collide between logger instances.

**Repair:** validate destination components and component names; create UUID-qualified log files exclusively, with restrictive Unix creation mode. Retain the file handle, flatten control characters, normalize the four primary levels, and fall back to safe stderr when file logging cannot work. Never emit raw tokens, authorization headers, proxy passwords or credential-bearing errors.

**Closure tests:** linked log directory creates no external files; fallback includes timestamps and levels; multiple instances do not collide; INFO/WARNING/ERROR/DEBUG records are all retained. MCP stdout remains reserved for protocol traffic.

## A06 — Medium: a successful installation can lack a discoverable SKILL.md

**Root cause:** entrypoint validation was case-insensitive, but writing preserved `skill.md`/`Skill.md`. Linux discovery requiring literal `SKILL.md` could not find the successfully installed package.

**Repair:** canonicalize the accepted root entrypoint to `SKILL.md` before constructing paths, without changing arbitrary nested filenames. Keep duplicate detection, case-folded path conflict checks, staging, limits and no-overwrite protections.

**Closure tests:** all three accepted spellings install the literal canonical entrypoint with original contents; case variants supplied twice are rejected; traversal, reserved names, file/directory conflicts and existing-package overwrites remain refused.

## A07 — Medium: explicit registration targets and dry-run validation are inconsistent

**Root cause:** `-CodeConfigPath` was ignored when that file lacked an Agensi entry, invoking the CLI against another user configuration instead. Code-target validation happened after dry-run; empty Desktop evidence could be reset rather than refused.

**Repair:** validate bounded object-shaped Desktop and Code documents before dry-run/mutation, reject empty/malformed files and identical targets, honor explicit Code paths by editing the requested document, and preserve unrelated settings and exact entries' additional fields. Continue allowing only explicit known-project upgrades and use same-directory replacement with a cooperative original-content check.

**Closure tests:** isolated Windows PowerShell covers legacy upgrade, unrelated-entry preservation/refusal, missing/empty-object/other-only explicit Code files, malformed Code dry-runs, and malformed Desktop refusal. No actual user Desktop/Code configuration is touched by these fixtures.

**Boundary:** two different client files are not a transactional filesystem. A late external change can still produce a partial operation; it must be reported and reconciled without overwriting the external edit.

## A08 — Validation gap: metadata checking is not advisory scanning

The previous dependency-review workflow validated Cargo metadata only. The first audit revision added the official pinned GitHub dependency-review action; an actual CI run showed Dependency Graph is disabled for this repository. Leaving that unsupported gate red or suppressing its failure would not establish security coverage.

This revision adds mandatory, pinned **cargo-audit 0.22.2** scanning of the full lockfile with a fresh project-local RustSec database, without ignored advisories or platform filters. It also denies unsoundness advisories. Run it on PRs, main pushes, manual dispatch and weekly schedules. Existing Cargo and GitHub Actions Dependabot coverage is retained. The GitHub-specific vulnerability diff remains optional, enabled only by `ENABLE_GITHUB_DEPENDENCY_REVIEW=true` after the account capability is enabled; mandatory RustSec does not depend on that setting.

Review actual new-SHA RustSec output, including any nonblocking maintenance/yank warnings. Fix genuine vulnerabilities rather than weakening gates. Existing full four-platform compilation/tests, pinned action SHAs, publication checks and CodeQL remain required.

## Research references

- [Official MCP Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)
- [Tokio asynchronous read contract](https://docs.rs/tokio/latest/tokio/io/trait.AsyncReadExt.html)
- [Agent Skills specification](https://agentskills.io/specification)
- [Rust symlink metadata contract](https://doc.rust-lang.org/std/fs/fn.symlink_metadata.html)
- [RustSec cargo-audit source and CLI](https://github.com/rustsec/rustsec/tree/cargo-audit/v0.22.2/cargo-audit)
- [Official dependency-review action](https://github.com/actions/dependency-review-action)
- [Spec Kit core reference](https://github.github.io/spec-kit/reference/core.html)

These references support implementation choices; they do not replace verification against the pinned dependencies or the owner's Rules. No runtime dependency upgrade, default HTTP-port change, credential deletion or background retry policy is bundled into this repair.
