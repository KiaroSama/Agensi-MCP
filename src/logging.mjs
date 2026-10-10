import { mkdirSync, openSync, writeSync, closeSync } from 'node:fs';
import { resolve } from 'node:path';
import { root } from './config.mjs';

export function logger(name) {
  let fd;
  const stamp = new Date().toISOString().slice(0, 19).replace('T', '_').replaceAll(':', '-');
  try {
    mkdirSync(resolve(root, 'logs'), { recursive: true });
    fd = openSync(resolve(root, 'logs', `${name}_${stamp}_UTC_${process.pid}.log`), 'wx', 0o600);
  } catch { process.stderr.write('Logging unavailable; status will use stderr.\n'); }
  return {
    // Callers pass status metadata only, never protocol bodies or raw dependency errors.
    write(level, message) {
      const line = `[${new Date().toISOString().slice(0, 19).replace('T', ' ')} UTC] [${level}] [${name}] ${message}\n`;
      try { if (fd !== undefined) writeSync(fd, line, null, 'utf8'); }
      catch { process.stderr.write('File logging failed.\n'); }
    },
    close() { if (fd !== undefined) { closeSync(fd); fd = undefined; } }
  };
}
