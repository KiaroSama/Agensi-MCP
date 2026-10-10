# Agensi Policies, Rights and Conflicts

Public-source baseline checked **2026-10-10**. Refresh official pages/current offer before consequential decisions. This is operating guidance, not legal/tax advice or guaranteed entitlement.

## Source map

| Source | Scope / baseline |
|---|---|
| [Terms](https://agensi.io/terms) | Updated2026-09-02: account, credits, purchases, creator rights/review, payouts, licenses |
| [Stripe payment terms](https://agensi.io/stripe-terms) | Updated2026-06-24: older payment/refund wording; conflicts below |
| [Privacy](https://agensi.io/privacy) | Updated2026-08-11: data categories/processors/retention; cloud task gaps |
| [Pricing](https://agensi.io/pricing), [MCP](https://agensi.io/mcp) | Current commercial/tools guidance; unversioned live pages |
| [Sell](https://agensi.io/sell), [Creator guide](https://agensi.io/learn/how-to-sell-skills-on-agensi) | Public creator positioning/workflow; older article can lag Terms |
| [Security](https://agensi.io/security), [Creator checklist](https://agensi.io/learn/skill-md-creator-checklist), [DMCA](https://agensi.io/dmca) | Scan signals, packaging advice and intellectual-property process |

A page's sitemap update or CDN response date does not establish a new policy version. Prefer the actual dated clause and applicable checkout/service response; surface contradictory sources.

## Access types

- **Free skill:** full `get_skill` content documented as public. Price zero does not grant redistribution rights or establish open-source license.
- **One-time paid:** card purchase or credits, permanent personal/nontransferable use and creator updates documented; local instructions available after ownership. Do not promise perpetual hosting or team/resale rights.
- **Credit-plan subscription:** periodic wallet credits, not ownership of all products and not the same as a particular skill subscription.
- **Monthly skill subscription:** cloud-only `run_skill`, result returned without protected instructions. No local-install promise or permanent ownership inference.
- **Trial:** three cloud runs/calendar month, one per skill advertised. Inspect actual allowance/response; consumption and data egress need approval even if free.

Baseline pricing page: Starter$9/month50 credits (rollover<=15), Pro$19/month120 (rollover<=35); top-ups base$0.20/credit, subscriber discounts10%/20%. Terms state paid skill credits=dollar price×5. These are dated observations, not current checkout quotations.

## Creator readiness and publication

Terms require original/distributable rights, valid skill package, safe truthful content and review. Creator retains IP and grants platform nonexclusive worldwide distribution/display/delivery license. /sell rejects repackaged free skills: licensing alone does not guarantee acceptance.

Paid/subscription publication requires enabled payout. Current Terms support Stripe Connect or Zoneless USDC-on-Solana; Stripe takes precedence when both active. /sell states payout KYC. Country eligibility, tax documents and actual verified account state must come from provider onboarding/current account evidence. A dashboard screenshot cannot replace a successful readiness result.

Baseline public submission UI: paid/monthly minimum$5, name<=60, summary/description and ZIP required; this is frontend evidence, not exhaustive backend schema. MCP files submission has its own exposed limits. Use actual tool validation and meaningful rights/safety checks rather than relying on UI limits alone.

Review advertised24–48h is an estimate, not SLA. Version changes queue for approval with prior live version serving buyers; public UI suggests listing-only edits can apply immediately. Do not say every edit is pending or every submission is live.

## Payout economics — only with actual transactions

Terms baseline: creator70%/platform30% of net excluding VAT/sales tax, processing included; Agensi merchant of record handles buyer tax/refunds and self-bills creator. Creator remains responsible for own income/tax obligations. Direct Stripe schedule and monthly credit earnings differ; Zoneless settlement/waits/holds apply.

Do not multiply public installs by price and70% to fabricate revenue. That calculation lacks transaction counts, realized prices, dates, payment route, discounts/refunds and taxes. A real earnings estimate requires approved private transaction evidence, which stays out of public research and reusable skill packages.

Never request a wallet private key, seed phrase or card credentials in chat. Payout onboarding is the provider's authorized human flow.

## Buyer rights and acquired content

Terms baseline: personal, nontransferable use; no redistribution/resale/sublicensing/sharing purchased contents. Buyer fingerprints in downloads must remain intact. Signed downloads documented as expiring24h with regenerated dashboard links; expired link is not lost ownership.

Keep ownership/license notices. Obtain explicit actual license terms before team sharing, republishing free content, redistributing outputs that may reproduce instructions, or adapting someone else's paid skill into a new sale. Research public metadata and write original guidance; do not clone protected instruction sets or reverse-engineer cloud-only delivery to obtain them.

## Refunds and expiry — unresolved official differences

| Topic | Official evidence at baseline | Operating consequence |
|---|---|---|
| One-time refund | Terms5.5:30-day full refund if it does not work for any reason, abuse exception; Stripe terms:14-day defect/misdescription, not guaranteed | Cite both; ask provider before promising entitlement or that refund has been issued |
| Credit refund | Terms4.7:30-day unused plan/top-up portion minus spent credits | Check actual purchase/usage and provider decision; no automatic refund guarantee |
| Top-up expiry | Pricing12months; Terms90days | Do not assign expiry without applicable offer/account/support evidence |
| Authentication/payment | MCP OAuth/card OR credits; older Terms API-key/credit-only | Use actual exposed schema/provider URL, not legacy guessed arguments |
| Free creator payouts | Older guide says Stripe even for free; updated Terms/current UI paid-specific and Zoneless alternative | Paid readiness requires enabled payout; free eligibility still verified from current service |
| Subscription rights/refunds | General permanent-acquisition wording vs monthly cloud-only behavior | Require specific subscription terms; do not apply one-time terms uncritically |
| Creator pricing/bundles | April pricing article suggests$3 entry/no formal bundles; September Terms minimum$5 and bundle support | Current Terms/actual offer supersede stale article advice; illustrative sales tables aren't transaction evidence |

Provider contact for documented refunds: info@agensi.io. Share a sanitized transaction reference through the authorized channel, not full local logs/auth state.

## Privacy and external execution

Privacy describes uploaded ZIP content/metadata/version/scan collection; MCP queries/previews/installs/channel logs retained2years; financial/payout records7years; downloads/fingerprints listing life+2years; deletion30days subject exceptions. It names Supabase, Stripe, Resend, Zoneless and Solana RPC processors.

The public policy does **not** establish cloud `run_skill` task-input/output retention, inference provider, training-use exclusion, network sandbox or zero-data-retention guarantee. Obtain explicit cloud-input approval and use sanitized data. Local transport does not mean local inference; a skill's own external API can add another processor/cost.

Privacy says no third-party analytics while baseline homepage HTML references analytics providers. This is a policy/deployment discrepancy, not proof of actual cookie use/transmission. Do not invent a privacy assurance or accuse a specific data leak from HTML alone.

## Scan and package guidance

Agensi scan describes structure/types/binaries, dangerous commands, secrets, environment harvesting, network access, obfuscation and prompt injection; advertised scores80–100Pass,60–79Warn,<60Fail. Exact implementation/rules are not public. Scan pass is neither safety proof, admin approval nor accuracy guarantee.

For a future authorized marketplace ZIP, prefer one top-level matching skill folder containing SKILL.md and necessary scripts/references/assets. Current UI describes root-or-one-folder-deep and50MB; these are not backend guarantees. Minimal SKILL.md can be format-valid; optional supporting files are advice, not a reason to add decorative files.

[Agent Skills standard](https://agentskills.io/specification) requires valid name/description frontmatter; [Claude Code docs](https://code.claude.com/docs/en/skills) govern its local extensions. `allowed-tools` preapproves tools in Code: it is NOT a restrictive sandbox/deny boundary despite contrary Agensi article wording. Host permissions and the user's actual consent control actions.
