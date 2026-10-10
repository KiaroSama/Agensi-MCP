import { readFileSync, mkdirSync, writeFileSync, renameSync, unlinkSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomUUID } from 'node:crypto';

export const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const configPath = resolve(root, 'config.local.json');
export const initial = () => ({ schema: 1, mode: 'disconnected', selected: null, profiles: [] });
const schemes = new Set(['http:', 'https:', 'socks4:', 'socks4a:', 'socks5:', 'socks5h:']);

export function proxyUrl(value) {
  let url;
  try { url = new URL(value); } catch { throw new Error('Invalid proxy URL.'); }
  const explicitPort = /^\w+:\/\/(?:[^/@]*@)?(?:\[[^\]]+\]|[^/:]+):(\d+)(?:\/)?$/.exec(value)?.[1];
  if (!schemes.has(url.protocol) || !url.hostname || !explicitPort ||
      Number(explicitPort) < 1 || Number(explicitPort) > 65535 ||
      (url.pathname && url.pathname !== '/') || url.search || url.hash) {
    throw new Error('Proxy needs a supported scheme, host and explicit port; no path or query.');
  }
  try { decodeURIComponent(url.username); decodeURIComponent(url.password); }
  catch { throw new Error('Invalid percent-encoded proxy credentials.'); }
  if (url.protocol.startsWith('socks4') && url.password) {
    throw new Error('SOCKS4 supports a user ID, not password authentication.');
  }
  return url;
}

export function validate(config) {
  if (!config || config.schema !== 1 || !['disconnected', 'direct', 'proxy'].includes(config.mode) ||
      !Array.isArray(config.profiles) || config.profiles.length > 100 ||
      !(config.selected === null || typeof config.selected === 'string')) {
    throw new Error('Invalid route settings; connection blocked.');
  }
  const names = new Set();
  for (const profile of config.profiles) {
    if (!profile || typeof profile.name !== 'string' || !/^[\w .-]{1,80}$/.test(profile.name) ||
        profile.name !== profile.name.trim() || names.has(profile.name.toLowerCase())) {
      throw new Error('Proxy names must be unique, 1-80 plain characters.');
    }
    proxyUrl(profile.url);
    names.add(profile.name.toLowerCase());
  }
  if (config.mode === 'proxy' && !config.profiles.some(p => p.name === config.selected)) {
    throw new Error('Selected proxy is missing; connection blocked.');
  }
  return config;
}

export function load(path = configPath) {
  let text;
  try { text = readFileSync(path, 'utf8'); }
  catch (error) { if (error.code === 'ENOENT') return initial(); throw new Error('Cannot read route settings.'); }
  let config;
  try { config = JSON.parse(text); } catch { throw new Error('Route settings are not valid JSON; connection blocked.'); }
  return validate(config);
}

export function save(config, path = configPath) {
  validate(config);
  mkdirSync(dirname(path), { recursive: true });
  const temp = `${path}.${randomUUID()}.tmp`;
  try {
    writeFileSync(temp, `${JSON.stringify(config, null, 2)}\n`, { encoding: 'utf8', mode: 0o600, flag: 'wx' });
    renameSync(temp, path);
  } finally { try { unlinkSync(temp); } catch (error) { if (error.code !== 'ENOENT') throw error; } }
}

export function publicStatus(config) {
  return { mode: config.mode, selected: config.selected, profiles: config.profiles.map(p => {
    const url = proxyUrl(p.url);
    return { name: p.name, protocol: url.protocol.slice(0, -1), host: url.hostname, port: Number(url.port || (url.protocol === 'https:' ? 443 : 80)), authenticated: Boolean(url.username) };
  }) };
}
