import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { parseProxyList, importProxyList } from '../src/proxy-import.mjs';
import { initial, load, save } from '../src/config.mjs';
import { spawnSync } from 'node:child_process';

const row = 'socks5://127.0.0.1:1080:test-user:test-pass:UK 5';
test('named list preserves final profile name and raw credentials', () => {
  const parsed = parseProxyList(`﻿${row}\r\n\r\nhttps://localhost:443:user:pa@ss%word:HTTPS\n`);
  assert.equal(parsed.length, 2);
  assert.equal(parsed[0].name, 'UK 5');
  const url = new URL(parsed[1].url);
  assert.equal(decodeURIComponent(url.password), 'pa@ss%word');
  assert.equal(parsed[1].name, 'HTTPS');
});

test('import is additive, skips duplicates and never changes selected route', () => {
  mkdirSync(resolve('.tmp'), { recursive: true });
  const dir = mkdtempSync(resolve('.tmp/import-'));
  const settings = join(dir, 'settings.json'), list = join(dir, 'Proxy list.txt');
  try {
    const config = initial();
    config.profiles = [{ name: 'existing', url: 'http://127.0.0.1:8080' }];
    config.mode = 'proxy'; config.selected = 'existing'; save(config, settings);
    writeFileSync(list, `${row}\n${row}\n`, 'utf8');
    assert.deepEqual(importProxyList(`"${list}"`, settings), { added: 1, skipped: 1 });
    assert.equal(load(settings).selected, 'existing');
    assert.equal(load(settings).mode, 'proxy');
    assert.deepEqual(importProxyList(list, settings), { added: 0, skipped: 2 });
    const before = readFileSync(settings, 'utf8');
    writeFileSync(list, `${row}\nsocks5://127.0.0.1:1080:test-user:SECRET:existing\n`, 'utf8');
    assert.throws(() => importProxyList(list, settings), error => /Line 2/.test(error.message) && !error.message.includes('SECRET'));
    assert.equal(readFileSync(settings, 'utf8'), before);
    writeFileSync(list, `${row}\nnot-a-proxy SECRET\n`, 'utf8');
    assert.throws(() => importProxyList(list, settings), /Line 2/);
    assert.equal(readFileSync(settings, 'utf8'), before);
    writeFileSync(list, Buffer.from([0xff, 0xfe, 0xfd]));
    assert.throws(() => importProxyList(list, settings), /UTF-8/);
    assert.equal(readFileSync(settings, 'utf8'), before);
  } finally { rmSync(dir, { recursive: true }); }
});

test('PowerShell menu preview is human-readable and offers file import', { timeout: 8000 }, () => {
  const result = spawnSync('pwsh', ['-NoLogo', '-NoProfile', '-NonInteractive', '-File', resolve('run.ps1'), '-Preview'],
    { encoding: 'utf8', timeout: 6000, windowsHide: true, cwd: resolve('.tmp') });
  assert.equal(result.status, 0, 'PowerShell preview must succeed from another working directory.');
  assert.match(result.stdout, /Agensi MCP Main menu:/);
  assert.match(result.stdout, /1\. Import proxy list from file \[1\]/);
  assert.match(result.stdout, /Connection:/);
  assert.equal(result.stdout.includes('"profiles"'), false);
});
