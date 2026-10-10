import { readFileSync } from 'node:fs';
import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
import { EventEmitter } from 'node:events';
import { configPath } from './config.mjs';
import { installPolicy } from './routing.mjs';

const snapshot = readFileSync(configPath, 'utf8');
const config = JSON.parse(snapshot);
if (config.mode === 'disconnected') throw new Error('Agensi is disconnected; select a proxy in run.ps1.');
const policy = installPolicy(config, Number(process.env.AGENSI_CALLBACK_PORT), () => {
  try { return readFileSync(configPath, 'utf8') === snapshot; } catch { return false; }
});
const timer = setInterval(() => {
  let changed = false;
  try { changed = readFileSync(configPath, 'utf8') !== snapshot; } catch { changed = true; }
  if (changed) {
    clearInterval(timer);
    policy.close().finally(() => process.exit(75));
  }
}, 200);
timer.unref();

// OAuth opens must be human initiated; the bridge's opener is the only subprocess user.
childProcess.spawn = () => {
  const child = new EventEmitter();
  child.pid = undefined;
  child.kill = () => false;
  queueMicrotask(() => child.emit('error', new Error('Automatic browser opening disabled.')));
  return child;
};
syncBuiltinESMExports();
