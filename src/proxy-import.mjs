import { readFileSync, statSync } from 'node:fs';
import { resolve } from 'node:path';
import { TextDecoder } from 'node:util';
import { configPath, load, save, validate, proxyUrl } from './config.mjs';

export function parseProxyList(text) {
  const profiles = [];
  const lines = text.replace(/^﻿/, '').split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (!line) continue;
    try {
      const match = /^([a-z0-9]+):\/\/(\[[^\]]+\]|[^:\s/]+):(\d+):([^:]*):([^:]*):([^:]+)$/i.exec(line);
      if (!match) throw new Error('Expected scheme://host:port:username:password:name.');
      const [, scheme, host, port, username, password, name] = match;
      const auth = username || password ? `${encodeURIComponent(username)}:${encodeURIComponent(password)}@` : '';
      const url = `${scheme.toLowerCase()}://${auth}${host}:${port}`;
      const profile = { name: name.trim(), url };
      validate({ schema: 1, mode: 'disconnected', selected: null, profiles: [profile] });
      profiles.push({ ...profile, line: i + 1 });
    } catch { throw new Error(`Line ${i + 1}: invalid proxy format, name or credentials; nothing imported.`); }
    if (profiles.length > 10000) throw new Error('Proxy list exceeds 10000 entries.');
  }
  if (!profiles.length) throw new Error('Proxy list contains no entries.');
  return profiles;
}

export function importProxyList(file, settingsPath = configPath) {
  let text;
  try {
    const path = resolve(file.trim().replace(/^(["'])(.*)\1$/, '$2'));
    if (statSync(path).size > 1024 * 1024) throw new Error('Too large.');
    text = new TextDecoder('utf-8', { fatal: true }).decode(readFileSync(path));
  } catch { throw new Error('Cannot read proxy list. Use a UTF-8 text file of at most 1 MiB.'); }
  const existing = load(settingsPath);
  const config = { ...existing, profiles: [...existing.profiles] };
  const names = new Map(config.profiles.map(p => [p.name.toLowerCase(), p]));
  let added = 0, skipped = 0;
  for (const entry of parseProxyList(text)) {
    const previous = names.get(entry.name.toLowerCase());
    if (previous) {
      if (proxyUrl(previous.url).href !== proxyUrl(entry.url).href) {
        throw new Error(`Line ${entry.line}: profile name conflicts with an existing entry; nothing imported.`);
      }
      skipped++;
    } else {
      const profile = { name: entry.name, url: entry.url };
      config.profiles.push(profile); names.set(profile.name.toLowerCase(), profile); added++;
    }
  }
  validate(config);
  if (added) save(config, settingsPath);
  return { added, skipped };
}
