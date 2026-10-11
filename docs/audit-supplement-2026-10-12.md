**First read and fully obey the owner's actual Rules, hooks, project/global agent instructions, research policy, and prescribed Spec Kit chain. Never disable a hook, invent an approval, publish private rules, or reset existing owner work to claim completion.**

# Agensi MCP: implemented audit supplement and integration contract

Date: 2026-10-12, Europe/Sofia. Repository: https://github.com/KiaroSama/Agensi-MCP.

This supplement is stacked on PR #4, branch `audit/2026-10-12-runtime-invariants`, at commit `2feb965dc358e5f20839df727757cac89c2c8eb8`. The separate branch is necessary to review the remaining changes without replacing the concurrent PR's tested runtime fixes. The authoring assistant is authorized to publish review candidates, not to merge or close any PR or change main.

## Evidence baseline and scope

The original independent executable audit ran 95 cases: the original main-equivalent executable passed 75 and failed 20; PR #4's first executable passed 94 and failed 1. Failures are scenarios, not counts of independent bugs. The original evidence bundle identifies executable SHA-256 hashes and source SHAs. These historical results do not certify a new executable.

PR #4's current revision repaired its initial formatting failure and made independent RustSec review mandatory. Its CI run 38182579738, dependency review 38182579759, and CodeQL 38182579742 completed successfully. Preserve its route ownership, coherent snapshot/cancellation, bounded storage, logging, skill casing, and explicit registration-path repairs. Do not reapply obsolete baseline replacements or remove final formatting enforcement. The GitHub dependency graph is disabled; retain the independent RustSec check and existing documented conditional GitHub-specific additional review rather than silently skipping all vulnerability review.

This supplement adds executable implementation and regression coverage for the remaining callback authority, coordinated registration, selected profile, and release identity findings. The release-policy suite was run locally: 8 PASS. Python syntax checks also passed. Rust and native Windows PowerShell execution must be established by the supplemental PR's own hosted CI, not inferred from PR #4's green badge. Consult the PR's latest evidence comment for actual run results and final head SHA. No real production Agensi account, external-browser sign-in, client GUI, or unavailable private owner hook/Spec Kit stage is claimed tested here.

## F02: callback authority and framing hardening

Root cause: PR #4 bounds and frames the callback, isolates malformed peers, and checks state/issuer/PKCE, but never validates its Host header. The previous executable accepted a synthetic callback with foreign Host. This is an endpoint-hardening defect, **not a demonstrated OAuth bypass without valid state/code/issuer**.

Implementation: `native/src/callback_headers.rs` is called by `native/src/auth.rs::read_callback`. Require exactly one Host field equal to the configured `127.0.0.1:<port>`, allowing field-name casing and HTTP whitespace only. Reject malformed field names, embedded controls/non-ASCII header values, duplicate Host, any Transfer-Encoding, nonzero/duplicate Content-Length, and bytes already following the header terminator. HTTP/1.0 remains accepted only with the same explicit Host requirement. A connection closes after the single response; no request-body or pipelining support is claimed.

Retain the existing 16 KiB header bound, fragmented reads, per-peer timeout, overall authorization timeout, duplicate query protection, state/issuer validation, routed-client lifetime, and best-effort browser response after persistence. No secret callback URL or authorization code is logged by the new validator.

Regression coverage: three Rust test methods cover valid casing/HTTP versions and invalid authority/framing cases. The independent Python suite adds missing-Host, duplicate-Host, and mixed-case-Host cases to the original inventory. Foreign/missing/duplicate Host must yield 400 without a saved session and must not prevent a subsequent legitimate callback from completing. The intended expanded full inventory is 98 cases; a truncated suite or harness-aborted case is not a complete pass.

## F06: selected-profile registration and coordinated publication

Root causes: after validating both documents, PR #4 still writes one target before the other without coordinated restoration on a later failure. Its default Code location also ignores `CLAUDE_CONFIG_DIR`. Official Claude documentation places `.claude.json` inside the selected configuration directory.

Implementation: `scripts/register.ps1` uses `registration-storage.ps1` and preserves the existing switches. Honor explicit targets directly and choose the active Code profile when no explicit path is provided; never invoke an implicit-HOME Claude CLI to mutate another configuration. Validate both entire UTF-8 object-shaped documents, their mcpServers shape, ownership, and distinct targets before any dry-run success or real mutation. Preserve unrelated client settings and extra metadata/environment keys on accepted owned entries. Exact registrations remain byte-identical and foreign registrations remain refused.

The helper bounds client configs to 32 MiB, separately from the native runtime state's existing 1 MiB policy. It refuses linked/reparse paths, acquires cooperative per-target locks in deterministic order, captures exact original bytes, prepares unique stage/recovery files, protects their Windows ACLs before writing sensitive configuration bytes, checks snapshots before publication, and verifies the completed targets. Existing Windows destination security metadata is preserved by File.Replace. New target files inherit the protected stage ACL.

On a later failure, restore only earlier publications still equal to this operation's new bytes. Verify restored bytes. Never overwrite a noncooperating concurrent edit. Where safe restoration is impossible, retain private recovery bytes and report ERROR instead of success. Locks are synchronization metadata; recovery data is not disposable evidence. Reconcile a recovery incident in the authorized session without deleting concurrent user work. This cooperative protocol does not claim global atomicity against power loss, process termination, or every hostile filesystem race; a durable recovery journal is an optional separately specified improvement.

Both `scripts/test-registration.ps1` and `scripts/test-registration-storage.ps1` must pass on native Windows PowerShell 7. The second suite covers explicit missing targets/no CLI invocation, private ACLs, unrelated data, idempotence, selected profiles, malformed/foreign configs, injected second-publication failure, byte-for-byte rollback, and concurrent-edit preservation with retained recovery bytes. Every fixture is project-local/disposable; never point these tests at the owner's real client files.

## F09: release tag must identify the built package

Root cause: checking only vX.Y.Z syntax permits a valid-looking tag that differs from Cargo.toml's package version. No release was published to demonstrate this.

Implementation: `scripts/check-release.py` reads Cargo.toml with the Python standard-library TOML parser and requires exact `v` plus package version. Versioned CI release requires main; artifact-only checks/dispatch remain allowed on branches. `scripts/test-release.py` checks matching/mismatched tags, missing prefix, shell metacharacters, branch policy, local checks, and the existing stable-only release contract. Inputs pass through environment variables, never interpolation into executable shell text.

CI adds a release preflight before platform builds and makes publishing depend on that preflight plus all native jobs. Existing pinned actions, four-platform matrix, checksum/publication checks, and explicit release dispatch policy remain intact. This change does not authorize the audit assistant to dispatch or publish a release.

## CI and combined verification

The expanded Linux black-box fixture terminates all synthetic proxy/TLS/OAuth/MCP traffic on loopback, uses ephemeral fixture certificates supplied only to child processes, and writes sanitized case results. No global trust-store changes, production credentials, payments, or account operations are involved. Fixture keys and synthetic session files are not uploaded. Both Windows registration suites are blocking. Python bytecode is ignored so the existing UTF-8 publication checker never accidentally packages generated .pyc files.

Require the final code-bearing revision to pass:

- `cargo +1.99.0 fmt --all --check`, `cargo +1.99.0 clippy --locked --all-targets -- -D warnings`, `cargo +1.99.0 build --locked`, `cargo +1.99.0 test --locked --all-targets -- --test-threads=2`, and `cargo +1.99.0 test --locked --doc`.
- On Windows: `pwsh -NoProfile -File scripts/test-registration.ps1` and `pwsh -NoProfile -File scripts/test-registration-storage.ps1`, against the freshly built executable, with each exit status checked.
- On Linux: `python3 tests/audit/audit_regressions.py --source-root . --output .tmp/audit-results.json`, all 98 cases, zero failures; and `python3 scripts/test-release.py`.
- Publication/sensitive-pattern checks, retained four-platform CI, CodeQL, mandatory RustSec, and current Dependabot Cargo/Actions coverage. A missing scanner feed or pending run is not PASS.

Fix real additional defects exposed by these tests in the same session with a regression. If a fixture asserts the wrong contract, explain and repair the fixture against source and documented behavior, retaining negative/positive controls; never suppress a genuine failure or endlessly rerun an unchanged failing check.

## Mandatory authorized-reviewer instructions

1. Start with actual private Rules/hooks and current specifications. Follow the owner's configured Spec Kit route. For an unmet existing specification use the Rules-prescribed converge/implement route; for changed requirements use specify, clarify, plan, tasks, analyze, implement and required interview/constitution/checklist/review/convergence stages. These are agent-interface stages, not invented PowerShell executables. Do not reinitialize private planning or fabricate execution of unavailable rules.
2. Research current primary documentation and GitHub implementations; record dated sources, alternatives, decisions, tests, and evidence in the locations required by the Rules. Do not publish excluded private research/specification directories.
3. **All prior and future published raw author and committer emails must be exactly `Kiaro.Sama.Dev@gmail.com`.** Audit and reconstruct incompatible history immediately under the Rules. A git config change, mailmap, or trailer does not fix existing raw commit fields. This connector cannot set identity fields and defaults to the account noreply identity, so its proposed commits require authorized reconstruction before final integration. Make a verified private recovery bundle, preserve content/topology and genuine provenance, map old/new SHAs, coordinate affected refs, use exact leases, and re-run checks after reconstruction. Never blindly force-push or overwrite newer owner work. The audit assistant has not rewritten shared history.
4. Review the existing PR #4 **and the supplemental PR containing this document**, each actual current diff and the combined tree. Prefer these existing review branches; do not create further branches by habit. The authorized reviewer can integrate the supplement into #4 then #4 into main, or reconcile/retarget in the correct order under the Rules. The authoring assistant must not merge or close either PR.
5. Inventory **all repository PRs**, including newly arriving ones, and apply the owner's Rules to every PR. Merge directly mergeable verified repairs; otherwise manually reconcile and integrate the equivalent correct implementation, verify it, then close the superseded PR under the Rules. A closed accepted-fix PR without its repair integrated is not DONE. All PRs must reach the Rules-defined merged/closed DONE state; do not exempt PRs merely as unrelated.
6. Fix genuine new discoveries immediately, retain exact error/recovery evidence, finish the combined acceptance gates, commit and push with the required email, and leave no known actionable repair half-integrated. Report actual external blockers honestly instead of claiming success. Finally provide default-branch SHA, raw identity audit, actual executed Rules/Spec Kit/hooks, finding-to-code-to-test ledger, final CI/native/Windows/black-box results, and disposition/integrating SHA for every PR.

Optional improvements for owner evaluation: a durable registration crash-recovery journal, redacted diagnostic summary, explicit registration status/dry-run diff limited to the owned entry, property tests for proxy/path parsing, and additional Windows reparse/ACL cases. Do not use these options to defer the required repairs or introduce unrelated runtimes, retry layers, or GPU dependencies into an I/O-bound connector.

## Primary references

- Existing PR and original implemented runtime audit: https://github.com/KiaroSama/Agensi-MCP/pull/4 and https://github.com/KiaroSama/Agensi-MCP/blob/audit/2026-10-12-runtime-invariants/docs/runtime-audit-2026-10-12.md
- Claude profile/user configuration: https://code.claude.com/docs/en/mcp-quickstart and https://code.claude.com/docs/en/settings
- HTTP/1.1 authority/framing: https://www.rfc-editor.org/rfc/rfc9112.html
- OAuth/state semantics: https://www.rfc-editor.org/rfc/rfc6749.html
- Rust SDK: https://github.com/modelcontextprotocol/rust-sdk ; use the lockfile's actual SDK version.
- File replacement contract: https://learn.microsoft.com/en-us/dotnet/api/system.io.file.replace
- Git commit identity fields: https://docs.github.com/en/rest/git/commits#create-a-commit ; the API supports identity fields but this connector wrapper does not expose them.
- Spec Kit: https://github.com/github/spec-kit ; actual installed owner Rules decide the chain.
- RustSec: https://github.com/RustSec/rustsec and https://github.com/RustSec/advisory-db
