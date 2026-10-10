# Agensi Operator

Original GPL-3.0-only operating guidance for Agensi: task search/selection, competitive demand analysis, skill optimization, safe account/install/trial/creator workflows and fail-closed connection handling. It is not affiliated with Agensi and does not grant service access or authorize purchases/publication.

## Claude Code

Load the local plugin for one new session:

```bash
claude --plugin-dir /absolute/path/to/packages/agensi-operator
```

Invoke `/agensi-operator:agensi-operator`. Alternatively copy the contained `skills/agensi-operator` folder to the target project's `.claude/skills/agensi-operator` for a standalone skill. Verify actual discovery; a folder on disk is not proof of loading.

## Codex and Cursor

Codex standalone path: target project's `.agents/skills/agensi-operator`; invoke `$agensi-operator` after discovery. The Codex native compatibility manifest is included; native marketplace registration is a separate user-approved operation, not performed by this package. Codex IDE supports standalone skills rather than plugins.

Cursor standalone path: `.cursor/skills/agensi-operator` or `.agents/skills/agensi-operator`; choose one to avoid duplicates. Native local testing uses the client's approved local plugin directory, subject to organizational policy. Its compatibility manifest and portable root are included; no global install is automatic.

## Antigravity

Use the standalone skill in `.agents/skills/agensi-operator`. Its native plugin root differs from Agent Plugins; use the separate `../agensi-operator-antigravity` package for project native installation in `.agents/plugins/agensi-operator`. Do not reuse this portable root as an Antigravity-native manifest.

## Permissions and rights

No MCP declarations, hooks, executable helpers or broad tool preapproval are bundled. Use your configured Agensi bridge. Cloud trial data, spending, installation/overwrite, media uploads, submissions and account/payout changes each need explicit action scope. Purchased marketplace contents retain their own rights; this original public guidance does not redistribute them.

## Evidence

See [evaluation method and limits](../../docs/operator-evaluation.md). Public install data does not prove sales, revenue or causality. Paired output experiment evidence is separate from native activation and live service testing. Native loading of all four clients has not been verified.

Copyright2026 Kiaro Sama. Licensed under [GNU GPLv3 only](LICENSE). Sources cited in references retain their own licenses; only original guidance is distributed here.

Official formats checked2026-10-10: [Claude](https://code.claude.com/docs/en/plugins), [Codex](https://developers.openai.com/codex/plugins/build), [Cursor](https://cursor.com/docs/reference/plugins), [Antigravity](https://antigravity.google/docs/plugins).
