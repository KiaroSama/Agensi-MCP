import { createHash } from 'node:crypto';
import { mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { root } from './config.mjs';

export const serverUrl = 'https://mcp.agensi.io/mcp';
export const serverHash = createHash('md5').update(serverUrl).digest('hex');
export const callbackPort = 3335 + parseInt(serverHash.slice(0, 4), 16) % 45816;
export const authRoot = resolve(root, '.state/auth');

export function childEnvironment() {
  for (const folder of [authRoot, resolve(root, '.tmp')]) mkdirSync(folder, { recursive: true });
  const env = { ...process.env, MCP_REMOTE_CONFIG_DIR: authRoot,
    AGENSI_CALLBACK_PORT: String(callbackPort), TEMP: resolve(root, '.tmp'), TMP: resolve(root, '.tmp') };
  for (const key of Object.keys(env)) {
    if (/^(https?_proxy|all_proxy|no_proxy|node_options|node_tls_reject_unauthorized)$/i.test(key)) delete env[key];
  }
  return env;
}

export function authorizationUrl(line) {
  const value = line.trim().replace(/^\[\d+\]\s*/, '');
  try {
    const url = new URL(value);
    if (url.protocol === 'https:' && !url.username && !url.password &&
        url.searchParams.get('response_type') === 'code' && url.searchParams.has('client_id')) return value;
  } catch {}
  return null;
}
