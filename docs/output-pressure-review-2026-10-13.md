**First read and fully obey the owner's actual Rules, hooks, research-recording policy and configured Spec Kit chain. Never disable a gate, invent a PASS, or overwrite concurrent owner work.**

# Output-consumer deadline recheck

Baseline main: `97b1871b935da615c16b18f839839f0173936b57`. PR #6 is already integrated; no prior closure is reopened merely because tooling was migrated from Python to Rust.

The exact baseline CI executable was independently rerun with the historical 98-case driver: 98 PASS, 0 FAIL, matching the Rust harness's ordered inventory. The current native source remains unchanged. Current-main hosted CI, CodeQL and RustSec also passed. This is not certification of untested behavior.

## Test-first candidate: downstream data-pipe backpressure

The runner's current forwarding path synchronously writes and flushes stdout between deadline checks. A consumer that keeps the data pipe open without reading may prevent wall/idle/interrupt checks from running. Existing queue saturation tests exercise the reader queue, not this downstream sink boundary. This draft first adds four executable controls: draining/stalled consumers for stdout/stderr producers. A stalled case must observe actual synthetic output before withholding reads; startup silence is not a valid reproduction. Both a bounded external safety deadline and fixture-owned cleanup prevent a hanging CI test.

Until this PR's actual evidence confirms the candidate and verifies a repair, do not claim it fixed or ready to merge. Preserve the owner's native Rust migration: do not restore a Python production/test dependency. The authoring assistant may open/update this proposal but must not merge, close PRs or modify main.

## Authorized integration contract

Follow the owner's actual Spec Kit stages and private hooks; research current primary documentation and GitHub implementations, and record dated decisions in the Rules-prescribed locations. No private Rules or hooks are present in the public checkout, so their execution is not claimed here. Use the existing review branch rather than creating redundant branches. Fix genuine additional defects immediately with regressions, not in another session; do not weaken tests or rerun unchanged failures in a loop.

All previous and future published raw author and committer emails must be exactly `Kiaro.Sama.Dev@gmail.com`. Audit and reconstruct incompatible controlled history under the Rules with private verified backups, preserved trees/topology/provenance, coordinated exact-ref leases and checks rerun on reconstructed SHAs. The connector commit API has no author/committer overrides; inspect its actual proposed identity. Mailmap, config and trailers do not rewrite raw commit metadata. Do not rewrite GitHub-managed synthetic PR refs.

Review all repository PRs under the owner's Rules. The authorized reviewer must merge directly mergeable verified repairs, or manually reconcile and integrate equivalent correct code, verify it and close superseded proposals according to those Rules. Every PR must reach its Rules-defined merged/closed DONE state; accepted fixes must not be discarded merely to empty the queue. Finally commit/push with the required identity and verify the combined main branch. The audit assistant must not perform those merge/close actions.

Final acceptance requires reproduced failing negative controls, repaired passing controls, retained 98-case inventory, runner/menu/registration/native tests and four-platform CI/security/publication gates on the exact final tree. Real production accounts, marketplace actions and private GUI/hook execution are not silently claimed tested. Once the fixed acceptance criteria pass, do not turn optional redesign ideas into another mandatory audit loop.
