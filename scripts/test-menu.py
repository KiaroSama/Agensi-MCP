"""Exercise the Windows launcher with isolated configuration and bounded input."""
from __future__ import annotations

import json
import logging
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
LOG = logging.getLogger('menu-tests')


class MenuTests(unittest.TestCase):
    def setUp(self):
        (ROOT / '.tmp').mkdir(exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(prefix='menu input-', dir=ROOT / '.tmp')
        self.root = Path(self.directory.name)
        (self.root / 'bin').mkdir()
        for path in ('run.ps1', 'Cargo.toml', 'bin/agensi-mcp.exe'):
            shutil.copy2(ROOT / path, self.root / path)
        LOG.info('Starting %s with disposable client state.', self._testMethodName)

    def tearDown(self):
        self.directory.cleanup()

    def launch(self, text: str, *options: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            ['pwsh', '-NoLogo', '-NoProfile', '-File', str(self.root / 'run.ps1'), *options],
            cwd=self.root / 'bin', input=text.encode('utf-8'), capture_output=True, timeout=10,
        )

    def test_advertised_quit_alias_exits(self):
        self.assertEqual(self.launch('quit\n').returncode, 0)

    def test_existing_exit_aliases_remain_supported(self):
        for choice in ('exit', '0'):
            with self.subTest(choice=choice):
                self.assertEqual(self.launch(choice + '\n').returncode, 0)

    def test_eof_exits_without_repeating_default_action(self):
        result = self.launch('')
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout.count(b'Agensi MCP Main menu:'), 1)
        self.assertFalse((self.root / 'config.local.json').exists())
        self.assertIn(b'[INFO] [menu] Input closed', result.stderr)

    def test_invalid_selection_then_eof_is_bounded(self):
        result = self.launch('not-a-choice\n')
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout.count(b'Agensi MCP Main menu:'), 2)
        self.assertFalse((self.root / 'config.local.json').exists())

    def test_blank_line_still_selects_default_import(self):
        # Import saves a synthetic profile only; it never contacts this endpoint.
        (self.root / 'Proxy list.txt').write_text('http://127.0.0.1:32123:::fixture\n', encoding='utf-8')
        result = self.launch('\n\nexit\n')
        self.assertEqual(result.returncode, 0)
        value = json.loads((self.root / 'config.local.json').read_text(encoding='utf-8'))
        self.assertEqual(value['mode'], 'disconnected')
        self.assertEqual(value['profiles'][0]['name'], 'fixture')
        self.assertEqual(len(value['profiles']), 1)

    def test_status_switch_keeps_machine_readable_output(self):
        result = self.launch('', '-Status')
        self.assertEqual(result.returncode, 0)
        value = json.loads(result.stdout)
        self.assertEqual(value['mode'], 'disconnected')
        self.assertEqual(value['profiles'], [])


if __name__ == '__main__':
    logging.basicConfig(level=logging.INFO, format='%(asctime)s [%(levelname)s] [menu-tests] %(message)s')
    if os.name != 'nt':
        LOG.error('Native Windows and a freshly built executable are required; no pass claimed.')
        raise SystemExit(2)
    unittest.main(verbosity=2)
