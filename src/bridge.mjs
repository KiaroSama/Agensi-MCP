import { spawn } from 'node:child_process';
import { resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';
import { serverUrl, callbackPort, childEnvironment, authorizationUrl } from './runtime.mjs';
import { root, load } from './config.mjs';
import { logger } from './logging.mjs';

const log = logger('agensi-bridge');
log.write('INFO', `Startup pid=${process.pid} node=${process.version} platform=${process.platform}`);
let child;
try {
  const config = load();
  if (config.mode === 'disconnected') throw new Error('Agensi is disconnected. Select a proxy using run.ps1.');
  const signIn = process.argv.includes('--signin');
  const entry = signIn ? resolve(root, 'src/signin.mjs') : resolve(root, 'node_modules/mcp-remote/dist/proxy.js');
  child = spawn(process.execPath, ['--import', pathToFileURL(resolve(root, 'src/preload.mjs')).href, entry,
    serverUrl, String(callbackPort), '--transport', 'http-only', '--auth-timeout', '300'],
  { cwd: root, env: childEnvironment(), windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  const deadline = signIn ? setTimeout(() => {
    process.stderr.write('Sign-in timed out. Run Sign in again; no direct fallback used.\n');
    child.kill();
  }, 310000) : null;
  if (!signIn) process.stdin.pipe(child.stdin);
  child.stdin.on('error', () => {});
  child.stdout.pipe(process.stdout);
  let authNext = false;
  const lines = createInterface({ input: child.stderr });
  lines.on('line', line => {
    if (line.includes('Please authorize this client by visiting:')) { authNext = true; return; }
    if (authNext) {
      const value = line.trim().replace(/^\[\d+\]\s*/, '');
      if (!value) return;
      authNext = false;
      try {
        const url = authorizationUrl(value);
        if (url) {
          process.stderr.write(`Copy this complete Agensi sign-in link into your external browser:\n${url}\nKeep this window open until approval completes.\n`);
          log.write('INFO', 'Human sign-in required; authorization URL omitted from log.');
        }
      } catch { log.write('WARNING', 'Unrecognized authorization URL; inspect client auth instructions.'); }
      return;
    }
    if (line.includes('AGENSI_SESSION_SAVED')) {
      process.stderr.write('Sign-in complete. Session saved locally for Desktop and Code.\n');
      log.write('INFO', 'OAuth session saved; token values omitted.');
    } else if (line.includes('AGENSI_SIGNIN_FAILED')) {
      process.stderr.write('Sign-in failed. Check proxy connectivity, callback port and service OAuth support. No session claimed.\n');
      log.write('ERROR', 'Sign-in failed; raw diagnostics omitted.');
    } else if (line.includes('Proxy established successfully')) {
      process.stderr.write('Agensi connected through the selected route.\n');
      log.write('INFO', 'Streamable HTTP bridge connected.');
    } else if (line.includes('Fatal error:')) {
      process.stderr.write('Agensi connection failed; check the selected proxy and authentication.\n');
      log.write('ERROR', 'Upstream bridge failed; raw error omitted for credential safety.');
    }
  });
  for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => child.kill());
  if (!signIn) process.stdin.on('end', () => child.kill());
  const code = await new Promise((resolveCode, reject) => {
    child.once('error', reject);
    child.once('exit', value => resolveCode(value ?? 1));
  });
  if (deadline) clearTimeout(deadline);
  lines.close();
  process.stdin.unpipe(child.stdin);
  process.stdin.pause();
  log.write(code === 0 ? 'INFO' : 'ERROR', `Shutdown exit=${code}`);
  if (code === 75) process.stderr.write('Route changed. Reconnect Agensi to use the new route.\n');
  process.exitCode = code;
} catch (error) {
  process.stderr.write(child ? 'Cannot start Agensi bridge.\n' : `${error.message}\n`);
  log.write('ERROR', 'Startup failed; connection blocked.');
  process.exitCode = 1;
} finally { log.close(); }
