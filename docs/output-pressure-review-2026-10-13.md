**First read and fully obey the owner's actual Rules, global/project agent instructions, hooks, research-recording policy and configured Spec Kit chain. Never disable a gate, invent approval or a PASS, expose private instructions, or overwrite concurrent owner work.**

# Agensi MCP: output-consumer deadline review and implemented repair

Audit date: 2026-10-13, Europe/Sofia. Repository: https://github.com/KiaroSama/Agensi-MCP. Review proposal: https://github.com/KiaroSama/Agensi-MCP/pull/8.

Baseline main: `97b1871b935da615c16b18f839839f0173936b57`. PR #6 and the owner's subsequent Rust-tooling migration PR #7 were already integrated. This proposal preserves that migration; no Python production/test dependency is restored. The audit assistant may open/update proposals, not merge or close PRs, modify main or publish a release.

## Baseline and preserved closures

Current-main CI `38430424626`, CodeQL `38430424615` and RustSec `38430424606` completed successfully. The exact current-main CI runtime executable has SHA-256 `564f9f6d071534e0348bbb01b5c4b880c02f72892581282236b1c2ed7e220e13`. Its checksum and source-commit file were verified. The new Rust black-box report contains 98 PASS, 0 FAIL. The historical independent driver was additionally executed outside the repository against that exact executable: 98 PASS, 0 FAIL, with identical ordered case identities. Historical code was used only as independent audit evidence, not reintroduced into the project.

The retained Linux ordinary-test log contains 92 passing methods: the prior 78 native methods plus eight release-policy and six operator-policy methods. Do not count those eight and six twice. The custom runner and black-box inventories are separate. Prior OAuth lifetime, route revocation, callback authority, bounded storage, registration and menu closures are not reopened without a new reproduction. The production `native/` source is unchanged by this repair.

## B1 — P2: downstream pipe pressure prevents deadline enforcement

**Location:** `tools/check/runner.rs`, `OutputEvent::Chunk` forwarding.

**Root cause:** the runner reads child stdout/stderr in bounded chunks but synchronously calls stdout `write_all` and `flush` inside the same loop that enforces wall/idle deadlines and interruption. A consumer that keeps the output pipe open but stops reading fills its buffer. The forwarding call blocks, preventing the next deadline check and owned-process cleanup. An input-reader queue saturation test does not exercise this downstream write boundary.

This is one newly reproduced liveness defect with two stream scenarios, not two unrelated bugs, and not a claim that the earlier newline/UTF-8 fixes regressed. It affects developer check tooling, not the Agensi MCP authentication or forwarding path.

### Actual before evidence

The test-first commit `9257345432b5f67ce07d69cefc29956c9954b7a1` changed only tests, CI evidence and this report, retaining the original runner. Its CI run is `38440714683`.

On Linux, both draining controls passed. Both stalled controls observed actual producer bytes but failed to exit by the external safety deadline: approximately 7260 ms and 7240 ms, despite the requested wall=4 seconds and idle=2 seconds. Closing the fixture consumer then released the blocked write for cleanup. A separate local invocation of the captured original `check-run` likewise observed output and remained running after 2.5 seconds despite wall=1 and idle=0.5. The baseline was not inferred from source alone.

The four-case regression deliberately distinguishes an active stalled pipe from an unrelated startup failure: each stalled test must first read the synthetic byte `x`. Both status stderr and cleanup are independently managed. A command that fails before producing output cannot pass the stalled control.

### Implemented correction

`tools/check/output.rs` introduces a small native data-output adapter. The existing runner keeps its two readers, 16 KiB chunk size, 64-slot bounded queue, command ownership, finite argument validation and cancellation mechanisms. No new thread, watchdog process, service or dependency is added.

For Unix FIFO/socket output, `output_unix.rs` obtains an owned duplicate through Rust's safe descriptor API and temporarily adds `O_NONBLOCK`, retaining the original flags. For Windows pipe output, `output_windows.rs` obtains an owned duplicate, reads its current mode and temporarily adds `PIPE_NOWAIT` without replacing its read mode. Failure to establish bounded pipe behavior fails the check rather than silently falling back to a blocking pipe. These APIs support inherited anonymous pipes as well as named pipes. This is synchronous nonblocking I/O with deadline checks, not an overlapped-I/O implementation.

The adapter tracks the exact unwritten suffix. Partial writes advance only by the reported byte count; Interrupted retries without losing bytes; WouldBlock and Windows zero-progress writes retain the existing budget and use a short bounded scheduling pause. Each attempt checks wall time, output-idle time and interruption. Only actual positive writes count as delivered progress. No unbounded `write_all` or consumer-draining pipe flush is used in the pipe path. Broken pipes remain failures, not replay or success.

Normal files and consoles retain their existing output behavior, including Windows console encoding. The original pipe mode is restored explicitly after command shutdown and again best-effort on guard drop if needed. A restoration error prevents a successful result. Interrupted stalled writes enter the same owned cleanup path and retain exit 130; a deadline failure remains a sanitized ERROR/nonzero exit. Already-sent bytes cannot be recalled, and an interrupted output is not reported complete.

**Boundaries:** the acceptance contract covers downstream data pipes whose consumer stops reading while the status/error sink remains usable. It is not a guarantee against arbitrary hung kernel devices/filesystems, permanently blocked status sinks, hostile concurrent descriptor replacement or uninterruptible OS execution. Those broader guarantees would require a separately specified execution architecture, not a hidden claim in this repair.

## Regression and CI coverage

`scripts/test-output-pressure.ps1` contains four independently bounded controls: draining stdout producer, draining stderr producer, stalled stdout producer and stalled stderr producer. Draining controls require all 2 MiB plus `END` and successful exit. Stalled controls require observed data and nonzero termination before an external seven-second safety deadline while retaining the original wall=4/idle=2 settings. Cleanup closes only fixture-owned handles and uses a fixture-owned process-tree kill as an emergency backstop. No account, proxy, token or real client configuration is involved. Timestamped INFO/WARNING/ERROR/DEBUG logging omits payloads and argument vectors.

Four pure Rust unit methods additionally verify byte-exact partial-write handling across WouldBlock/Interrupted, bounded zero-progress writes, cancellation before another write, and a broken sink that is not retried. The existing 18 runner identities, six native Windows menu contracts, both registration suites and 98 black-box identities remain intact. The new pressure step is blocking on all four native CI platforms. Formatting, Clippy, locked builds, metadata test-target mapping, CodeQL, RustSec and publication checks remain mandatory.

The evidence collector now retains the developer runner and its checksum, sanitized pressure JSON and a `git archive` of committed public source alongside the existing runtime evidence. Ignored runtime state, credentials, private hooks, planning and caches are not included. Release archives and explicit release authorization are unchanged. Existing Cargo and Actions Dependabot coverage is retained; no dependency change is required.

**Implementation-stage status:** before-fix reproduction is established. This document is published with the repair before its final hosted verification. Consult PR #8's latest final-evidence comment and downloadable audit report for the actual tested final SHA, run IDs, result counts and checksums. Do not treat an older green main badge, a pending run or a platform-skipped step as this repair passing. Local Rust compilation/native Windows PowerShell are unavailable in the audit container; their actual execution is established through the PR's hosted CI, not fabricated locally.

## Authorized reviewing-agent completion instructions

1. Read the owner's real Rules, hooks and current specifications before touching integration. Follow the actual configured Spec Kit chain. Use the Rules-prescribed converge/implement path for an unmet existing acceptance criterion; use specify, clarify, plan, tasks, analyze and implement, with any required interview/constitution/checklist/review gates, only when the Rules call for requirement changes. These are agent stages, not invented shell commands. Do not reinitialize or publish excluded private planning. This audit does not claim unavailable private stages were executed.
2. Research current repository context, official documentation and primary GitHub implementations, including the references below. Record dated sources, tradeoffs, tests and adopted/rejected decisions in the Rules-prescribed files. Keep optional improvement decisions separate from mandatory fixes.
3. **All previous and future published raw author and committer emails must be exactly `Kiaro.Sama.Dev@gmail.com`. Audit and immediately reconstruct incompatible controlled history under the owner's Rules.** The current-main head already has the required fields. The connector's commit action has no author/committer overrides; inspect the actual proposed commits and reconstruct incompatible ones before final integration. Preserve verified private recovery backups, trees, topology and genuine provenance, map old/new SHAs, coordinate refs with exact leases and rerun checks after reconstruction. A mailmap, config setting or trailer does not repair historical raw metadata. Never overwrite newer owner work or rewrite GitHub-managed synthetic PR refs.
4. Review the actual code already proposed in PR #8, not an obsolete Python rewrite or the original #4/#5/#6 patch set. Reuse this review branch; create another branch only when genuinely required by the Rules or integration constraints. Prefer PowerShell 7 in the existing owner checkout under `G:\Program Files\Portable\Scripts`; do not relocate it or add global runtimes unnecessarily. Preserve English comments and credential-free timestamped four-level logs.
5. Keep the fixed B1 closure ledger. Verify all four pressure controls and four Rust boundaries, the existing 18 runner methods, native Windows menu and registration suites, all 98 black-box cases and the combined four-platform/security/publication gates. Fix genuine additional defects exposed along the way immediately in the same session with regressions. Correct a mistaken diagnosis when evidence disproves it; never silence an assertion or rerun an unchanged failure indefinitely.
6. Inventory **all repository PRs**, including newly arriving proposals, and follow the owner's Rules for every one. Merge directly mergeable verified work; otherwise manually reconcile and integrate the equivalent correct implementation, verify it, and close superseded proposals according to those Rules. Every PR must reach its Rules-defined merged/closed DONE state. Closing accepted unfixed work merely to empty the queue is not DONE. These merge/close operations belong only to the authorized reviewer, never the audit assistant.
7. After actual Rules/hooks and acceptance gates pass, commit/push with the required identity, finish any necessary branch integration and validate the combined default branch. Report final main SHA, raw identity audit, actually executed Spec Kit stages/hooks, B1-to-code-to-test evidence and the disposition/integrating SHA of each PR. Leave no known accepted repair half-integrated. Record genuinely external blockers honestly rather than claiming they passed.

**Stop condition:** once B1's fixed acceptance controls and the retained combined suite pass, this repair is complete within that stated scope. Do not invent mandatory redesigns merely because universal defect-freedom cannot be proved. Production account/browser/GUI checks and private owner hooks are not silently certified.

## Optional, nonblocking improvements

Evaluate workflow-schema linting and artifact-retention/cost policy. Keep the native implementation and current process-ownership model unless measured evidence justifies a change. GPU work is unrelated to this I/O scheduling defect. These suggestions are decisions for the owner, not unfinished accepted repairs or prerequisites for another audit.

## Primary references

- Rust partial-write contract: https://doc.rust-lang.org/std/io/trait.Write.html
- Rust standard-output descriptor/handle APIs and console behavior: https://doc.rust-lang.org/std/io/struct.Stdout.html
- Windows pipe state preservation: https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-getnamedpipehandlestatew
- Windows bounded synchronous pipe mode: https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-setnamedpipehandlestate
- Windows partial/zero-byte nonblocking writes: https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-type-read-and-wait-modes
- Apple platform flag definitions: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/fcntl.h
- Owner's integrated Rust migration: https://github.com/KiaroSama/Agensi-MCP/pull/7
- Current-main validation: https://github.com/KiaroSama/Agensi-MCP/actions/runs/38430424626
- Test-first reproduction: https://github.com/KiaroSama/Agensi-MCP/actions/runs/38440714683
- Spec Kit, subordinate to the owner's installed Rules: https://github.com/github/spec-kit
