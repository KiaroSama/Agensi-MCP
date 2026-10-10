import { readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import http from 'node:http';
import { timingSafeEqual } from 'node:crypto';
import { auth } from '@modelcontextprotocol/client';
import { root } from './config.mjs';
import { serverUrl, serverHash, callbackPort, authRoot } from './runtime.mjs';

export function checkedCallback(url, expectedState) {
  const state = url.searchParams.get('state') || '';
  const a = Buffer.from(state), b = Buffer.from(expectedState || '');
  if (!a.length || a.length !== b.length || !timingSafeEqual(a, b)) throw new Error('Invalid authorization state.');
  if (url.searchParams.has('error') || !url.searchParams.get('code')) throw new Error('Authorization was not approved.');
  return { authorizationCode: url.searchParams.get('code'), iss: url.searchParams.get('iss') || undefined };
}

export async function manualSignIn({ endpoint = serverUrl, hash = serverHash, port = callbackPort,
  fetchFn = globalThis.fetch, discoverFn, emit = line => process.stderr.write(line) } = {}) {
  // The pinned bridge exports its durable provider; never duplicate its token/cache format.
  const folder = resolve(root, 'node_modules/mcp-remote/dist');
  const chunk = readdirSync(folder).find(p => p === 'chunk-CGVEK4Z7.js');
  if (!chunk) throw new Error('Pinned OAuth provider bundle changed; reinstall locked dependencies.');
  const { NodeOAuthClientProvider, discoverOAuthServerInfo } = await import(pathToFileURL(resolve(folder, chunk)));
  const discovered = await (discoverFn || discoverOAuthServerInfo)(endpoint, {});
  const provider = new NodeOAuthClientProvider({ serverUrl: discovered.authorizationServerUrl, resourceServerUrl: endpoint,
    callbackPath: '/oauth/callback', callbackPort: port, host: '127.0.0.1', clientName: 'Agensi Local',
    serverUrlHash: hash, fetchFn, authorizationServerMetadata: discovered.authorizationServerMetadata,
    protectedResourceMetadata: discovered.protectedResourceMetadata, wwwAuthenticateScope: discovered.wwwAuthenticateScope });
  const originalClientInfo = provider.clientInformation.bind(provider);
  const originalTokens = provider.tokens.bind(provider);
  const stamp = (value, filename, context) => {
    if (!value) return value;
    let issuer;
    try {
      issuer = JSON.parse(readFileSync(resolve(process.env.MCP_REMOTE_CONFIG_DIR || authRoot,
        'mcp-remote-v1', `${hash}_${filename}.json`), 'utf8')).issuer;
    } catch {}
    if (issuer && context?.issuer && issuer !== context.issuer) throw new Error('Cached authorization issuer mismatch.');
    return issuer ? { ...value, issuer } : value;
  };
  provider.clientInformation = async context => stamp(await originalClientInfo(), 'client_info', context);
  provider.tokens = async context => stamp(await originalTokens(), 'tokens', context);
  let discovery, expectedState, resolveCallback, rejectCallback;
  const callback = new Promise((resolveValue, reject) => { resolveCallback = resolveValue; rejectCallback = reject; });
  // Attach immediately so a bind failure cannot become an unhandled rejection.
  callback.catch(() => {});
  provider.saveDiscoveryState = async value => { discovery = value; };
  provider.discoveryState = async () => discovery;
  provider.redirectToAuthorization = async url => {
    expectedState = url.searchParams.get('state');
    emit(`Please authorize this client by visiting:\n${url.toString()}\n`);
  };
  const server = http.createServer((request, response) => {
    const url = new URL(request.url, `http://127.0.0.1:${port}`);
    if (url.pathname !== '/oauth/callback') { response.writeHead(404); response.end(); return; }
    try {
      const result = checkedCallback(url, expectedState);
      provider.useAuthorizationState(url.searchParams.get('state'));
      response.writeHead(200, { 'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'no-store' });
      response.end('Authorization received. Return to the Agensi menu to verify session storage.');
      resolveCallback(result);
    } catch {
      response.writeHead(400, { 'Content-Type': 'text/plain; charset=utf-8' }); response.end('Authorization callback rejected.');
      if (expectedState && url.searchParams.get('state') === expectedState && url.searchParams.has('error')) {
        rejectCallback(new Error('Authorization declined.'));
      }
    }
  });
  server.requestTimeout = 10000;
  const timer = setTimeout(() => rejectCallback(new Error('Authorization approval timed out.')), 300000);
  try {
    await new Promise((resolveValue, reject) => { server.once('error', reject); server.listen(port, '127.0.0.1', resolveValue); });
    const options = { serverUrl: endpoint, fetchFn: (input, init = {}) => fetchFn(input,
      { ...init, signal: AbortSignal.timeout(20000) }) };
    const result = await auth(provider, options);
    if (result === 'REDIRECT') {
      const received = await callback;
      if (await auth(provider, { ...options, ...received }) !== 'AUTHORIZED') throw new Error('Token exchange failed.');
    } else if (result !== 'AUTHORIZED') throw new Error('Authorization not completed.');
    const tokens = await provider.tokens();
    if (!tokens?.access_token) throw new Error('No saved usable OAuth session.');
    emit('AGENSI_SESSION_SAVED\n');
  } finally {
    clearTimeout(timer); server.closeAllConnections();
    await new Promise(resolveValue => server.close(resolveValue));
  }
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(root, 'src/signin.mjs')) {
  try { await manualSignIn(); }
  catch { process.stderr.write('AGENSI_SIGNIN_FAILED\n'); process.exitCode = 1; }
}
