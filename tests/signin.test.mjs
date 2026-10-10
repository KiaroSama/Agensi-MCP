import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { authorizationUrl, callbackPort, serverHash, childEnvironment, authRoot } from '../src/runtime.mjs';
import { checkedCallback } from '../src/signin.mjs';

test('full manual authorization URL stays unchanged and rejects non-auth URLs', () => {
  const url = 'https://example.test/oauth?client_id=test&response_type=code&state=opaque%2Bstate&code_challenge=abc%2Fxyz&redirect_uri=http%3A%2F%2F127.0.0.1';
  assert.equal(authorizationUrl(url), url);
  assert.equal(authorizationUrl(`[1234] ${url}`), url);
  assert.equal(authorizationUrl('https://example.test/'), null);
  assert.equal(authorizationUrl('https://user:password@example.test/?client_id=x&response_type=code'), null);
});

test('callback rejects wrong state and approval errors', () => {
  assert.throws(() => checkedCallback(new URL('http://localhost/oauth/callback?state=wrong&code=test'), 'expected'));
  assert.throws(() => checkedCallback(new URL('http://localhost/oauth/callback?state=expected&error=denied'), 'expected'));
  assert.deepEqual(checkedCallback(new URL('http://localhost/oauth/callback?state=expected&code=test&iss=https%3A%2F%2Fissuer.test'), 'expected'),
    { authorizationCode: 'test', iss: 'https://issuer.test' });
});

test('login and bridge share stable callback and project cache', () => {
  const env = childEnvironment();
  assert.equal(Number(env.AGENSI_CALLBACK_PORT), callbackPort);
  assert.equal(callbackPort, 3335 + parseInt(serverHash.slice(0, 4), 16) % 45816);
  assert.equal(env.MCP_REMOTE_CONFIG_DIR, authRoot);
  assert.equal(Object.keys(env).some(k => /^(https?_proxy|all_proxy|no_proxy)$/i.test(k)), false);
});

test('real menu reads input with colon and exposes sign-in', { timeout: 8000 }, () => {
  const result = spawnSync('pwsh', ['-NoLogo', '-NoProfile', '-File', resolve('run.ps1')],
    { input: 'exit\n', encoding: 'utf8', windowsHide: true, timeout: 6000 });
  assert.equal(result.status, 0);
  assert.match(result.stdout, /Selection \[1\] quit=exit: /);
  assert.match(result.stdout, /7\. Sign in \(copy link to external browser\)/);
  assert.equal(result.stdout.includes('socks5://'), false);
});

test('Windows preload uses file URL and reaches the application entry', { timeout: 5000 }, () => {
  // Exercise the real preload in an isolated root, never an owner's route or token state.
  mkdirSync(resolve('.tmp'), { recursive: true });
  const folder = mkdtempSync(resolve('.tmp/preload-'));
  mkdirSync(resolve(folder, 'src'));
  for (const name of ['preload.mjs', 'config.mjs', 'routing.mjs']) copyFileSync(resolve('src', name), resolve(folder, 'src', name));
  writeFileSync(resolve(folder, 'config.local.json'), JSON.stringify({ schema: 1, mode: 'direct', selected: null, profiles: [] }), 'utf8');
  try {
    const result = spawnSync(process.execPath, ['--import', pathToFileURL(resolve(folder, 'src/preload.mjs')).href,
      '--input-type=module', '-e', "console.log('PRELOAD_READY')"],
      { encoding: 'utf8', windowsHide: true, timeout: 3000, env: childEnvironment() });
    assert.equal(result.status, 0, result.error?.message || result.stderr);
    assert.match(result.stdout, /PRELOAD_READY/);
    assert.equal(result.stderr.includes('ERR_UNSUPPORTED_ESM_URL_SCHEME'), false);
  } finally { rmSync(folder, { recursive: true }); }
});

test('manual OAuth callback saves reusable session without displaying tokens', { timeout: 8000 }, async () => {
  const { mkdirSync, mkdtempSync, rmSync, readFileSync, readdirSync } = await import('node:fs');
  const { manualSignIn } = await import('../src/signin.mjs');
  const { createServer } = await import('node:net');
  mkdirSync(resolve('.tmp'), { recursive: true });
  const folder = mkdtempSync(resolve('.tmp/oauth-'));
  const previous = process.env.MCP_REMOTE_CONFIG_DIR;
  process.env.MCP_REMOTE_CONFIG_DIR = folder;
  const probe = createServer();
  await new Promise(r => probe.listen(0, '127.0.0.1', r));
  const port = probe.address().port;
  await new Promise(r => probe.close(r));
  const endpoint = 'https://oauth-fixture.test/mcp';
  let tokenExchanges = 0, callbacks = [];
  const output = [];
  const fakeFetch = async (input, init = {}) => {
    const url = String(input);
    if (url.includes('/.well-known/oauth-protected-resource')) return Response.json({ resource: endpoint,
      authorization_servers: ['https://oauth-fixture.test'], scopes_supported: ['read', 'install'] });
    if (url.includes('/.well-known/')) return Response.json({ issuer: 'https://oauth-fixture.test',
      authorization_endpoint: 'https://oauth-fixture.test/authorize', token_endpoint: 'https://oauth-fixture.test/token',
      registration_endpoint: 'https://oauth-fixture.test/register', response_types_supported: ['code'],
      grant_types_supported: ['authorization_code', 'refresh_token'], code_challenge_methods_supported: ['S256'],
      token_endpoint_auth_methods_supported: ['none'] });
    if (url.endsWith('/register')) return Response.json({ client_id: 'fixture-client', redirect_uris: [`http://127.0.0.1:${port}/oauth/callback`], token_endpoint_auth_method: 'none' });
    if (url.endsWith('/token')) { tokenExchanges++; return Response.json({ access_token: 'fixture-access', refresh_token: 'fixture-refresh', token_type: 'Bearer', expires_in: 3600 }); }
    return new Response('', { status: 401, headers: { 'WWW-Authenticate': 'Bearer' } });
  };
  try {
    await manualSignIn({ endpoint, hash: 'fixture', port, fetchFn: fakeFetch,
      discoverFn: async () => ({ authorizationServerUrl: 'https://oauth-fixture.test',
        protectedResourceMetadata: { resource: endpoint, authorization_servers: ['https://oauth-fixture.test'], scopes_supported: ['read', 'install'] },
        authorizationServerMetadata: await (await fakeFetch('https://oauth-fixture.test/.well-known/oauth-authorization-server')).json() }),
      emit: line => {
      output.push(line);
      const urlLine = line.split('\n').find(x => x.startsWith('https://'));
      if (urlLine) {
        const authorization = new URL(urlLine);
        const callback = new URL(authorization.searchParams.get('redirect_uri'));
        callback.searchParams.set('state', authorization.searchParams.get('state'));
        callback.searchParams.set('code', 'fixture-code');
        callbacks.push(fetch(callback, { signal: AbortSignal.timeout(2000) }).then(r => r.text()));
      }
    } });
    await Promise.all(callbacks);
    assert.equal(tokenExchanges, 1);
    assert.equal(output.join('').includes('fixture-access'), false);
    assert.match(output.join(''), /AGENSI_SESSION_SAVED/);
    const cache = readdirSync(folder).find(x => x.startsWith('mcp-remote'));
    const saved = JSON.parse(readFileSync(resolve(folder, cache, 'fixture_tokens.json'), 'utf8'));
    assert.equal(saved.access_token, 'fixture-access');
    const { NodeOAuthClientProvider } = await import('../node_modules/mcp-remote/dist/chunk-CGVEK4Z7.js');
    const bridgeProvider = new NodeOAuthClientProvider({ serverUrl: endpoint, resourceServerUrl: endpoint,
      serverUrlHash: 'fixture', callbackPath: '/oauth/callback', callbackPort: port, host: '127.0.0.1',
      protectedResourceMetadata: { resource: endpoint, authorization_servers: ['https://oauth-fixture.test'], scopes_supported: ['read', 'install'] } });
    const reused = await bridgeProvider.tokens();
    assert.equal(Boolean(reused?.access_token), true, 'Bridge must retain login token for discovered scopes.');
    assert.equal(saved.requested_scope, 'read install');
  } finally {
    if (previous === undefined) delete process.env.MCP_REMOTE_CONFIG_DIR; else process.env.MCP_REMOTE_CONFIG_DIR = previous;
    rmSync(folder, { recursive: true });
  }
});
