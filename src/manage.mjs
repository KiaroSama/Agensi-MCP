import { readFileSync, existsSync, lstatSync, mkdirSync, writeFileSync, renameSync, rmSync, mkdtempSync } from 'node:fs';
import { resolve, join, dirname, relative, isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';
import { root, load, save, publicStatus, proxyUrl } from './config.mjs';
import { logger } from './logging.mjs';
import { importProxyList } from './proxy-import.mjs';

export function saveSkill(name, files, skillsRoot = resolve(root, 'skills')) {
  if (!/^[a-zA-Z0-9][a-zA-Z0-9_-]{0,79}$/.test(name) || !Array.isArray(files) || !files.length || files.length > 500) {
    throw new Error('Invalid skill folder name or file list.');
  }
  for (let path = skillsRoot; ; path = dirname(path)) {
    if (existsSync(path) && lstatSync(path).isSymbolicLink()) throw new Error('Linked skill directory refused.');
    if (dirname(path) === path) break;
  }
  const target = join(skillsRoot, name);
  if (existsSync(target)) throw new Error('Skill folder already exists; nothing overwritten.');
  const names = new Set();
  let bytes = 0;
  for (const entry of files) {
    if (!entry || typeof entry.path !== 'string' || typeof entry.content !== 'string') throw new Error('Each file needs path and text content.');
    const parts = entry.path.split(/[\\/]/);
    if (isAbsolute(entry.path) || parts.some(p => !p || p === '.' || p === '..' || /[<>:"|?*\x00-\x1f]/.test(p) ||
        /[. ]$/.test(p) || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(p))) {
      throw new Error('Unsafe skill file path refused.');
    }
    const key = parts.join('/').toLowerCase();
    if (names.has(key)) throw new Error('Duplicate skill file path refused.');
    names.add(key);
    bytes += Buffer.byteLength(entry.content, 'utf8');
    if (bytes > 20 * 1024 * 1024) throw new Error('Skill text exceeds 20 MiB safety limit.');
  }
  if (!names.has('skill.md')) throw new Error('Skill package must include SKILL.md.');
  mkdirSync(skillsRoot, { recursive: true });
  const staging = mkdtempSync(join(skillsRoot, '.install-'));
  try {
    for (const entry of files) {
      const path = resolve(staging, ...entry.path.split(/[\\/]/));
      if (relative(staging, path).startsWith('..')) throw new Error('Skill path escaped installation root.');
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, entry.content, { encoding: 'utf8', flag: 'wx', mode: 0o600 });
    }
    if (existsSync(target)) throw new Error('Skill folder appeared concurrently; nothing overwritten.');
    renameSync(staging, target);
  } finally { if (existsSync(staging)) rmSync(staging, { recursive: true }); }
  return target;
}

export function command(args, input = '') {
  const [action, name, value] = args;
  if (action === 'import') return importProxyList(name || resolve(root, 'Proxy list.txt'));
  const config = load();
  if (action === 'status') return publicStatus(config);
  if (action === 'add') {
    const url = (value === '--stdin' ? input : value)?.trim();
    proxyUrl(url);
    if (config.profiles.some(p => p.name.toLowerCase() === name?.toLowerCase())) throw new Error('That proxy name already exists.');
    config.profiles.push({ name, url });
  } else if (action === 'select') {
    if (!config.profiles.some(p => p.name === name)) throw new Error('Unknown proxy profile.');
    config.mode = 'proxy'; config.selected = name;
  } else if (action === 'direct' || action === 'disconnect') {
    config.mode = action === 'direct' ? 'direct' : 'disconnected';
  } else if (action === 'save-skill') {
    let data;
    try { data = JSON.parse(readFileSync(value, 'utf8')); }
    catch { throw new Error('Cannot read a valid inline skill JSON file.'); }
    return { saved: saveSkill(name, Array.isArray(data) ? data : data.files) };
  } else { throw new Error('Use status, add, import, select, direct, disconnect or save-skill.'); }
  save(config);
  return publicStatus(config);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const log = logger('agensi-menu');
  try {
    let input = '';
    if (process.argv.includes('--stdin')) for await (const chunk of process.stdin) {
      input += chunk.toString('utf8');
      if (input.length > 8192) throw new Error('Proxy input too long.');
    }
    const output = command(process.argv.slice(2), input);
    log.write('INFO', 'Management action completed.');
    process.stdout.write(`${JSON.stringify(output, null, 2)}\n`);
  } catch (error) {
    log.write('ERROR', 'Management action failed; credential values omitted.');
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  } finally { log.close(); }
}
