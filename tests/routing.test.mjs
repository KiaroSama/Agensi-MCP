import test from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import https from 'node:https';
import { execFileSync } from 'node:child_process';
import { createRoute, installPolicy } from '../src/routing.mjs';
import net from 'node:net';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync, existsSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { load, save, initial, publicStatus, proxyUrl, validate } from '../src/config.mjs';
import { saveSkill } from '../src/manage.mjs';
import { fetch as sdkFetch, getGlobalDispatcher, setGlobalDispatcher } from 'undici';

const listen = server => new Promise((resolve, reject) => {
  server.once('error', reject);
  server.listen(0, '127.0.0.1', () => resolve(server.address().port));
});

test('selected dead proxy never contacts the destination', { timeout: 5000 }, async () => {
  let contacts = 0;
  const destination = http.createServer((_req, res) => { contacts++; res.end('direct bypass'); });
  const port = await listen(destination);
  const closed = http.createServer();
  const deadPort = await listen(closed);
  await new Promise(resolve => closed.close(resolve));
  const route = createRoute({ schema: 1, mode: 'proxy', selected: 'dead', profiles: [
    { name: 'dead', url: `http://127.0.0.1:${deadPort}` }
  ] });
  try {
    await assert.rejects(route.fetch(`http://127.0.0.1:${port}`, { signal: AbortSignal.timeout(1500) }));
    assert.equal(contacts, 0);
  } finally {
    await route.close();
    destination.closeAllConnections();
    await new Promise(resolve => destination.close(resolve));
  }
});

function track(server) {
  const sockets = new Set();
  server.on('connection', socket => { sockets.add(socket); socket.on('close', () => sockets.delete(socket)); });
  return async () => { for (const socket of sockets) socket.destroy(); await new Promise(r => server.close(r)); };
}

async function connectProxy(tls) {
  let tunnels = 0;
  const server = tls ? https.createServer(tls) : http.createServer();
  const close = track(server);
  server.on('connect', (req, client, head) => {
    tunnels++;
    const [host, port] = req.url.split(':');
    const target = net.connect(Number(port), host, () => {
      client.write('HTTP/1.1 200 Connection Established\r\n\r\n');
      if (head.length) target.write(head);
      target.pipe(client); client.pipe(target);
    });
    target.on('error', () => client.destroy());
    client.on('close', () => target.destroy());
  });
  return { port: await listen(server), close, count: () => tunnels };
}

async function socksProxy(type) {
  let tunnels = 0;
  const server = net.createServer(client => {
    let buffer = Buffer.alloc(0), stage = type === 5 ? 'greeting' : 'connect';
    const onData = chunk => {
      buffer = Buffer.concat([buffer, chunk]);
      if (stage === 'greeting') {
        if (buffer.length < 2 + buffer[1]) return;
        buffer = buffer.subarray(2 + buffer[1]);
        client.write(Buffer.from([5, 0])); stage = 'connect';
      }
      let host, port, consumed;
      if (type === 4) {
        const end = buffer.indexOf(0, 8);
        if (buffer.length < 9 || end < 0) return;
        port = buffer.readUInt16BE(2); host = [...buffer.subarray(4, 8)].join('.'); consumed = end + 1;
      } else {
        if (buffer.length < 5) return;
        const size = buffer[3] === 3 ? 1 + buffer[4] : 4;
        if (buffer.length < 4 + size + 2) return;
        host = buffer[3] === 3 ? buffer.subarray(5, 4 + size).toString('utf8') : [...buffer.subarray(4, 8)].join('.');
        port = buffer.readUInt16BE(4 + size); consumed = 6 + size;
      }
      tunnels++;
      client.off('data', onData);
      const target = net.connect(port, host, () => {
        client.write(type === 4 ? Buffer.from([0, 90, 0, 0, 0, 0, 0, 0]) : Buffer.from([5, 0, 0, 1, 127, 0, 0, 1, 0, 0]));
        if (buffer.length > consumed) target.write(buffer.subarray(consumed));
        target.pipe(client); client.pipe(target);
      });
      target.on('error', () => client.destroy());
      client.on('close', () => target.destroy());
    };
    client.on('data', onData);
  });
  const close = track(server);
  return { port: await listen(server), close, count: () => tunnels };
}

for (const protocol of ['http', 'socks4', 'socks5', 'socks5h']) {
  test(`${protocol} profile uses its proxy and closes cleanly`, { timeout: 5000 }, async () => {
    const destination = http.createServer((_req, res) => res.end('through proxy'));
    const closeDestination = track(destination);
    const port = await listen(destination);
    const proxy = protocol === 'http' ? await connectProxy() : await socksProxy(protocol.startsWith('socks4') ? 4 : 5);
    const route = createRoute({ schema: 1, mode: 'proxy', selected: 'test', profiles: [{ name: 'test', url: `${protocol}://127.0.0.1:${proxy.port}` }] });
    try {
      const response = await route.fetch(`http://127.0.0.1:${port}`, { signal: AbortSignal.timeout(2000) });
      assert.equal(await response.text(), 'through proxy');
      assert.equal(proxy.count(), 1);
      await route.close();
      await assert.rejects(route.fetch(`http://127.0.0.1:${port}`));
    } finally { await route.close(); await proxy.close(); await closeDestination(); }
  });
}

test('route state persists, credentials are hidden and invalid settings block', () => {
  mkdirSync(resolve('.tmp'), { recursive: true });
  const folder = mkdtempSync(resolve('.tmp/settings-'));
  const path = join(folder, 'config.json');
  try {
    assert.equal(load(path).mode, 'disconnected');
    const config = initial();
    config.profiles.push({ name: 'named', url: 'http://secret-user:secret-pass@127.0.0.1:80' });
    config.mode = 'proxy'; config.selected = 'named'; save(config, path);
    assert.deepEqual(load(path), config);
    assert.equal(JSON.stringify(publicStatus(config)).includes('secret'), false);
    assert.equal(publicStatus(config).profiles[0].port, 80);
    assert.throws(() => proxyUrl('socks4://user:pass@localhost:1080'));
    assert.throws(() => proxyUrl('http://localhost'));
    assert.throws(() => validate({ ...config, selected: 'missing' }));
    writeFileSync(path, '{"password":"DO_NOT_ECHO"', 'utf8');
    assert.throws(() => load(path), error => !error.message.includes('DO_NOT_ECHO'));
  } finally { rmSync(folder, { recursive: true }); }
});

test('inline skill files save exact UTF-8 and refuse escapes or overwrites', () => {
  mkdirSync(resolve('.tmp'), { recursive: true });
  const folder = mkdtempSync(resolve('.tmp/skills-'));
  try {
    const files = [{ path: 'SKILL.md', content: '# Skill\nUTF-8: café نمونه\n' }, { path: 'references/usage.md', content: 'Use locally.\n' }];
    const saved = saveSkill('example', files, folder);
    assert.equal(readFileSync(join(saved, 'SKILL.md'), 'utf8'), files[0].content);
    assert.throws(() => saveSkill('example', files, folder));
    for (const path of ['../outside.md', 'C:/outside.md', 'file:stream', 'NUL.txt', 'bad.']) {
      assert.throws(() => saveSkill('unsafe', [{ path: 'SKILL.md', content: 'safe' }, { path, content: 'no' }], folder));
      assert.equal(existsSync(join(folder, 'unsafe')), false);
    }
  } finally { rmSync(folder, { recursive: true }); }
});

test('global and SDK fetch obey policy and a route change blocks new traffic', { timeout: 5000 }, async () => {
  let contacts = 0;
  const destination = http.createServer((_req, res) => { contacts++; res.end('policy'); });
  const close = track(destination);
  const port = await listen(destination);
  const oldFetch = globalThis.fetch, oldDispatcher = getGlobalDispatcher();
  let unchanged = true;
  const policy = installPolicy({ schema: 1, mode: 'disconnected', selected: null, profiles: [] }, null, () => unchanged);
  try {
    await assert.rejects(globalThis.fetch(`http://127.0.0.1:${port}`));
    await assert.rejects(sdkFetch(`http://127.0.0.1:${port}`));
    unchanged = false;
    await assert.rejects(globalThis.fetch(`http://127.0.0.1:${port}`));
    assert.equal(contacts, 0);
  } finally { await policy.close(); globalThis.fetch = oldFetch; setGlobalDispatcher(oldDispatcher); await close(); }
});

test('TLS destination works through HTTPS and SOCKS with certificate validation', { timeout: 10000 }, async () => {
  mkdirSync(resolve('.tmp'), { recursive: true });
  const folder = mkdtempSync(resolve('.tmp/tls-'));
  const keyPath = join(folder, 'key.pem'), certPath = join(folder, 'cert.pem');
  try {
    execFileSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', keyPath,
      '-out', certPath, '-days', '1', '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost,IP:127.0.0.1'],
    { timeout: 5000, windowsHide: true, stdio: 'ignore', env: { ...process.env, MSYS_NO_PATHCONV: '1' } });
    const cert = readFileSync(certPath), key = readFileSync(keyPath);
    const destination = https.createServer({ cert, key }, (_req, res) => res.end('verified TLS'));
    const closeDestination = track(destination), port = await listen(destination);
    try {
      for (const protocol of ['https', 'socks4', 'socks5']) {
        const proxy = protocol === 'https' ? await connectProxy({ cert, key }) : await socksProxy(protocol === 'socks4' ? 4 : 5);
        const route = createRoute({ schema: 1, mode: 'proxy', selected: 'tls', profiles: [{ name: 'tls', url: `${protocol}://127.0.0.1:${proxy.port}` }] }, { ca: cert });
        try {
          const response = await route.fetch(`https://127.0.0.1:${port}`, { signal: AbortSignal.timeout(2000) });
          assert.equal(await response.text(), 'verified TLS');
          assert.equal(proxy.count(), 1);
        } finally { await route.close(); await proxy.close(); }
      }
    } finally { await closeDestination(); }
  } finally { rmSync(folder, { recursive: true }); }
});
