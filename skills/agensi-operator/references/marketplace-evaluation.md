# Find and Select a Skill That Actually Fits

Use for choosing an existing Agensi offer. For market/product opportunities read [market-demand.md](market-demand.md); for improving a draft read [skill-optimization.md](skill-optimization.md). Money/in-session-use operations use [tools-and-workflows.md](tools-and-workflows.md).

## 1. Turn the request into a search contract

Capture required input, observable deliverable, actual agent capabilities, mandatory limits and excluded actions. Separate "explain/plan" from "produce a usable file/patch". A skill claiming to access local files may not work in a cloud runner.

Example: "help with receipts" becomes "approved exported receipt records→monthly folder manifest and duplicate/unmatched report; preserve originals; no email forwarding/posting." That distinction changes both search and permission checks.

**Done:** a task contract whose output can be accepted/rejected.

## 2. Search in three passes

Use the live `search_skills` schema, public terms only:

| Pass | Query shape | Reason |
|---|---|---|
| Problem | "receipt reconciliation", "dependency upgrade", "style drift" | Find workflow intent |
| Artifact | "monthly expense report", "upgrade patch", "brand voice audit" | Find what actually ships |
| Constraint/integration | "CSV", "Elementor", "TypeScript", "Arabic documents" | Check exact niche/runtime/format |

Start relevance-ranked; compare free and paid candidates rather than rank solely by installs. Add popular/recent/request-board views only when demand is part of the question. If no hits, try one synonym/related category and inspect exact boundaries; avoid endless searches or installing unrelated offers.

Deduplicate by slug. For a normal selection, a few distinct alternatives are enough; broad research records query/date/page/call budgets and stopping reason. Overlapping pages, capped portfolios and historical opt-out metadata mean coverage is partial. `updated_at` alone does not prove maintained code; `get_popular(month)` plus cumulative counters does not establish monthly growth.

**Done:** a small shortlist or an evidence-backed "no demonstrated match." No commercial action yet.

## 3. Read details, then build the comparison

Call `get_skill(slug)` without confirmation. Inspect actual use cases, sample output, prerequisites, limitations, permissions, price/rights and accessible instruction evidence. Creator portfolio can provide private analysis context if relevant, but the report synthesizes lessons rather than names/biographies/directories. Paid preview-only instruction quality remains unknown.

| Candidate | Output fit | Free/simple alternative gap | Setup/data permissions | Quality proof / unknown | Total cost |
|---|---|---|---|---|---|
| Relevant A | Actual file/patch versus plan; format | What it adds beyond prompting | Host/network/API/accounts/rights | Listing / instruction / actual execution | Current price+external costs |

Name critical blockers, not a numeric "quality score": wrong format/language, unavailable tools, sensitive cloud input, unsupported API, missing runtime, incompatible license, vague or contradictory description. Missing fields mean unknown, not zero risk. A scan/verified badge is not tested competence.

**Done:** every shortlisted candidate has a supported strength, limitation/unknown and fit decision.

## 4. Prove the selected advantage at the cheapest valid tier

- If the free/basic alternative meets the contract, recommend it; don't create a paid need.
- If paid instructions are hidden, recommend at most a approval-dependent purchase, stating what remains untested. No unlocking as research.
- If inspection alone settles fit, stop. If output quality needs testing, define one actual/sanitized representative input, expected deliverable and meaningful error case before asking for run approval.
- For cloud: establish capability, destination/privacy, cost and exact data approval. For host execution: inspect loaded instructions/scripts and permissions; keep acquired content in-session rather than installing it.

Inspect produced output directly: rows/facts/units/citations, generated files/imports, rendered visual results or preserved meaning as appropriate. A successful API response or tool listing is not acceptance. If a run is blocked, describe the missing tool/input/service and remaining uncertainty, not a hollow example.

**Done:** chosen offer is demonstrated fit, conditional fit or rejected with specific evidence.

## 5. Decide and report

Recommend one candidate, not a popularity directory. Suggested output:

> **Decision:** use existing/free / approval-dependent paid use / narrow custom workflow / no fit.
> **Why:** decisive task-output advantage.
> **Evidence:** current source/date and listing/instruction/runtime depth.
> **Friction:** prerequisites, data destination, external costs and rights.
> **Next:** one authorized action or exact blocked check.

Choosing a future product to build instead? Route to market-demand and require buyer-gap/prototype evidence; a failed search alone is not market demand or permission to submit a request-board post.

## Extracted quality lessons from inspected offers

| Useful pattern | Apply it | Watch for |
|---|---|---|
| Clear severity/file/line/fix output | Make diagnostic results actionable and source-located | Sample crash/security claims depend on actual host context; validate rather than repeating |
| Explicit JSONL/schema/label deliverable | Define data shape and failure handling | Exact dedup is not semantic dedup; dataset generation is not an eval runner |
| Editable native implementation | Differentiate usable artifact from a prose plan | Rights, writes, dependencies and fidelity must be verified; "pixel-perfect" isn't evidence |
| Specific query taxonomy and boundaries | Decide which reasoning path applies | Architecture guidance isn't an implemented store; causal/counterfactual claims need evidence |
| Honest limits and missing prerequisites | Stop a bad-fit purchase early | Paid previews can still hide implementation; small review samples don't prove performance |

Public source examples checked2026-10-10: [review contract](https://agensi.io/skills/code-reviewer), [dataset contract](https://agensi.io/skills/prompt-dataset-builder), [editable implementation](https://agensi.io/skills/clone-site-to-elementor), [reasoning boundary](https://agensi.io/skills/temporal-reasoning-sleuth). No product was purchased/run; patterns are original synthesis, not copied instructions. Preserve [license/privacy distinctions](policies.md).
