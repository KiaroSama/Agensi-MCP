import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, lstatSync, existsSync } from 'node:fs';
import { resolve, relative, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const files = execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard', '-z'],
  { cwd: root, encoding: 'utf8', timeout: 5000 }).split('\0').filter(Boolean);
const forbidden = /(^|\/)(?:\.ai|\.specify|specs|\.claude|\.codex|\.agents|\.cursor|\.kiro|node_modules|logs|\.state|\.cache|\.tmp|graphify-out|\.codebase-memory)(\/|$)|(^|\/)(?:secrets\.md|explain-AI\.md|\.ignoreme|config\.local\.json|Proxy list\.txt|\.env(?:\..*)?)$/i;
const secret = /(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,}|sk-ant-[A-Za-z0-9_-]{20,}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----|cs_live_[A-Za-z0-9]{20,})/;
for (const name of files) {
  if (forbidden.test(name)) throw new Error(`Private/generated publication path refused: ${name}`);
  const path = resolve(root, name);
  if (lstatSync(path).isSymbolicLink()) throw new Error(`Linked publication file refused: ${name}`);
  const text = new TextDecoder('utf-8', { fatal: true }).decode(readFileSync(path));
  if (secret.test(text)) throw new Error(`Sensitive-pattern match: ${name}; value omitted.`);
  if (/\.(mjs|ps1|py)$/.test(name) && text.split('\n').length > 800) throw new Error(`Source line ceiling exceeded: ${name}`);
}
const bundle = resolve(root, 'packages/agensi-operator');
const skill = resolve(bundle, 'skills/agensi-operator');
const main = readFileSync(resolve(skill, 'SKILL.md'), 'utf8').replace(/\r\n/g, '\n');
if (!main.startsWith('---\n') || !main.includes('name: agensi-operator\n') || main.split('\n').length >= 500) throw new Error('Invalid main skill format.');
function checkLinks(folder) {
  for (const entry of readdirSync(folder, { withFileTypes: true })) {
    const path = resolve(folder, entry.name);
    if (entry.isSymbolicLink()) throw new Error('Linked resource refused.');
    if (entry.isDirectory()) { checkLinks(path); continue; }
    if (!entry.name.endsWith('.md')) continue;
    const text = readFileSync(path, 'utf8');
    // This inspects document prose, not a URL trust/hostname boundary.
    if (/agensi[.]io\/creators\//.test(text)) throw new Error('Creator directory is not distributable guidance.');
    for (const [, target] of text.matchAll(/\]\(([^)]+)\)/g)) {
      if (/^(https?:|#)/.test(target)) continue;
      const linked = resolve(dirname(path), target.split('#')[0]);
      if (relative(skill, linked).startsWith('..') || !existsSync(linked)) throw new Error(`Broken/escaping skill reference: ${entry.name}`);
    }
  }
}
checkLinks(skill);
for (const client of ['claude', 'codex', 'cursor']) {
  const manifest = JSON.parse(readFileSync(resolve(bundle, `.${client}-plugin/plugin.json`), 'utf8'));
  if (manifest.name !== 'agensi-operator' || manifest.skills !== './skills/' || manifest.license !== 'GPL-3.0-only') throw new Error(`Invalid ${client} manifest.`);
  if (manifest.hooks || manifest.mcpServers) throw new Error('Instruction plugin must not auto-connect or execute hooks.');
}
const alternative = resolve(root, 'packages/agensi-operator-antigravity');
const native = JSON.parse(readFileSync(resolve(alternative, 'plugin.json'), 'utf8'));
if (Object.keys(native).sort().join(',') !== 'description,name') throw new Error('Invalid native Antigravity root.');
for (const name of ['SKILL.md', ...readdirSync(resolve(skill, 'references')).map(n => `references/${n}`)]) {
  if (!readFileSync(resolve(skill, name)).equals(readFileSync(resolve(alternative, 'skills/agensi-operator', name)))) throw new Error('Client content mismatch.');
}
console.log(`Publication checks passed: ${files.length} UTF-8 source files; original plugin references and client parity valid.`);
