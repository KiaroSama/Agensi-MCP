"""Exercise owned-process cleanup, bounded streaming, and safe runner diagnostics."""
from __future__ import annotations

import ctypes
import logging
import io
import queue
import runpy
import threading
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
LOG = logging.getLogger('check-tests')


def alive(pid: int) -> bool:
    if os.name == 'nt':
        api = ctypes.WinDLL('kernel32', use_last_error=True)
        api.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
        api.OpenProcess.restype = ctypes.c_void_p
        api.GetExitCodeProcess.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32)]
        api.CloseHandle.argtypes = [ctypes.c_void_p]
        handle = api.OpenProcess(0x1000, False, pid)
        if not handle:
            return False
        try:
            code = ctypes.c_uint32()
            return bool(api.GetExitCodeProcess(handle, ctypes.byref(code))) and code.value == 259
        finally:
            api.CloseHandle(handle)
    try:
        state = Path(f'/proc/{pid}/stat')
        if state.exists() and state.read_text(encoding='utf-8').split(') ', 1)[1].startswith('Z'):
            return False
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


class RunnerTests(unittest.TestCase):
    def setUp(self):
        (ROOT / '.tmp').mkdir(exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(prefix='check streams-', dir=ROOT / '.tmp')
        self.fixture = Path(self.directory.name)
        self.project = self.fixture / 'project with spaces'
        (self.project / 'scripts').mkdir(parents=True)
        self.runner = self.project / 'scripts/run-check.py'
        shutil.copy2(ROOT / 'scripts/run-check.py', self.runner)
        LOG.info('Starting %s with disposable fixture data.', self._testMethodName)

    def tearDown(self):
        self.directory.cleanup()

    def invoke(self, command: list[str], wall='6', idle='3') -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, '-I', str(self.runner), f'--wall={wall}', f'--idle={idle}', '--', *command],
            cwd=self.fixture, capture_output=True, timeout=15,
        )

    def python(self, program: str, wall='6', idle='3') -> subprocess.CompletedProcess:
        return self.invoke([sys.executable, '-I', '-c', program], wall, idle)

    def test_nonzero_exit_is_preserved(self):
        self.assertEqual(self.python('raise SystemExit(7)').returncode, 7)

    def test_unterminated_stdout_is_progress(self):
        result = self.python("import os,time\nfor _ in range(20):\n os.write(1,b'x');time.sleep(.1)\nos.write(1,b'END')", idle='1')
        self.assertEqual(result.returncode, 0, 'Active byte output must not hit the idle deadline.')
        self.assertEqual(result.stdout, b'x' * 20 + b'END')

    def test_unterminated_stderr_is_progress(self):
        result = self.python("import os,time\nfor _ in range(20):\n os.write(2,b'x');time.sleep(.1)\nos.write(2,b'END')", idle='1')
        self.assertEqual(result.returncode, 0, 'Active stderr must not require newline framing.')
        self.assertEqual(result.stdout, b'x' * 20 + b'END')

    def test_non_utf8_stdout_is_preserved(self):
        result = self.python("import os;os.write(1,b'begin\\xffend\\n')")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b'begin\xffend\n', 'Reader must not die on non-UTF-8 bytes.')
        self.assertNotIn(b'Traceback', result.stderr)

    def test_non_utf8_stderr_is_preserved(self):
        result = self.python("import os;os.write(2,b'begin\\xffend\\n')")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b'begin\xffend\n', 'Both command streams share the byte-preserving sink.')
        self.assertNotIn(b'Traceback', result.stderr)

    def test_split_multibyte_sequence_is_preserved(self):
        result = self.python("import os,time;os.write(1,b'\\xe2');time.sleep(.1);os.write(1,b'\\x82\\xacEND')")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b'\xe2\x82\xacEND')

    def test_nonfinite_and_nonpositive_limits_never_start_command(self):
        marker = self.fixture / 'started'
        program = f'from pathlib import Path;Path({str(marker)!r}).touch()'
        for field in ('wall', 'idle'):
            for value in ('nan', 'inf', '-inf', '0', '-1'):
                with self.subTest(field=field, value=value):
                    marker.unlink(missing_ok=True)
                    result = self.python(program, **{field: value})
                    self.assertFalse(marker.exists(), 'Invalid limits must fail before process creation.')
                    self.assertNotEqual(result.returncode, 0)

    def test_large_unterminated_output_finishes_without_truncation(self):
        result = self.python("import os\nfor _ in range(256):os.write(1,b'x'*8192)\nos.write(1,b'END')")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(len(result.stdout), 2 * 1024 * 1024 + 3)
        self.assertTrue(result.stdout.endswith(b'END'))

    def test_true_idle_check_fails_within_bound(self):
        started = time.monotonic()
        result = self.python('import time;time.sleep(6)', wall='4', idle='.5')
        self.assertNotEqual(result.returncode, 0)
        self.assertLess(time.monotonic() - started, 5)
        self.assertIn(b'TimeoutError', result.stderr)

    def test_wall_deadline_remains_independent_of_progress(self):
        started = time.monotonic()
        result = self.python("import os,time\nfor _ in range(100):\n os.write(1,b'x\\n');time.sleep(.05)", wall='.8', idle='3')
        self.assertNotEqual(result.returncode, 0)
        self.assertLess(time.monotonic() - started, 5)

    def descendant_case(self, inherit_output: bool):
        lease = self.fixture / 'fixture.lease'
        lease.touch()
        pid_file = self.fixture / 'grandchild.pid'
        child = ("from pathlib import Path\nimport sys,time\nend=time.monotonic()+8\n"
                 "while Path(sys.argv[1]).exists() and time.monotonic()<end:time.sleep(.02)\n")
        output_args = '' if inherit_output else ',stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL'
        parent = ("import subprocess,sys\nfrom pathlib import Path\n"
                  f"p=subprocess.Popen([sys.executable,'-I','-c',{child!r},{str(lease)!r}],stdin=subprocess.DEVNULL{output_args})\n"
                  f"Path({str(pid_file)!r}).write_text(str(p.pid),encoding='utf-8')\n")
        pid = None
        try:
            result = self.python(parent, wall='4', idle='.8' if inherit_output else '2')
            pid = int(pid_file.read_text(encoding='utf-8'))
            end = time.monotonic() + 3
            while alive(pid) and time.monotonic() < end:
                time.sleep(.02)
            self.assertFalse(alive(pid), 'An owned descendant survived runner cleanup.')
            if inherit_output:
                self.assertNotEqual(result.returncode, 0)
            else:
                self.assertEqual(result.returncode, 0)
        finally:
            # A unique cooperative lease avoids targeting a recycled process identifier.
            lease.unlink(missing_ok=True)
            if pid is None and pid_file.exists():
                pid = int(pid_file.read_text(encoding='utf-8'))
            end = time.monotonic() + 3
            while pid and alive(pid) and time.monotonic() < end:
                time.sleep(.02)
            self.assertFalse(pid and alive(pid), 'Fixture cleanup did not complete.')

    def test_success_reaps_descendant_after_direct_parent_exit(self):
        self.descendant_case(False)

    def test_idle_timeout_reaps_descendant_holding_pipes(self):
        self.descendant_case(True)

    def test_linked_log_directory_is_not_written(self):
        outside = self.fixture / 'outside log destination'
        outside.mkdir()
        link = self.project / 'logs'
        if os.name == 'nt':
            subprocess.run(['cmd', '/d', '/c', 'mklink', '/J', str(link), str(outside)],
                           check=True, capture_output=True, timeout=5)
        else:
            link.symlink_to(outside, target_is_directory=True)
        try:
            result = self.python('pass')
            self.assertEqual(result.returncode, 0, 'Safe stderr fallback should remain usable.')
            self.assertEqual(len(list(outside.iterdir())), 0, 'Logger wrote through a linked directory.')
            self.assertIn(b'[WARNING]', result.stderr)
            self.assertIn(b'[INFO]', result.stderr)
        finally:
            if os.name == 'nt':
                link.rmdir()
            else:
                link.unlink()

    def test_own_logs_omit_command_arguments_and_output_payload(self):
        sentinel = 'fixture-private-payload-not-a-real-secret'
        result = self.python(f'print({sentinel!r})')
        self.assertEqual(result.returncode, 0)
        self.assertIn(sentinel.encode(), result.stdout)
        logs = list((self.project / 'logs').glob('check_*.log'))
        self.assertEqual(len(logs), 1)
        text = logs[0].read_text(encoding='utf-8')
        self.assertNotIn(sentinel, text)
        self.assertRegex(text, r'\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} UTC\] \[INFO\]')

    def test_full_queue_reader_stops_without_deadlock(self):
        module = runpy.run_path(str(self.runner))
        self.assertGreater(module['PENDING_CHUNKS'], 0)
        self.assertLessEqual(module['PENDING_CHUNKS'] * module['CHUNK_BYTES'], 1024 * 1024)
        events = queue.Queue(maxsize=1)
        stop = threading.Event()
        stream = io.BytesIO(b'x' * (module['CHUNK_BYTES'] * 4))
        worker = threading.Thread(target=module['consume_output'], args=(stream, events, stop), daemon=True)
        worker.start()
        try:
            end = time.monotonic() + 2
            while not events.full() and time.monotonic() < end:
                time.sleep(.01)
            self.assertTrue(events.full())
        finally:
            stop.set()
            worker.join(timeout=2)
        self.assertFalse(worker.is_alive(), 'A full output queue prevented owned cleanup.')

    def test_reader_failure_is_distinct_from_eof_and_redacted(self):
        module = runpy.run_path(str(self.runner))
        class BrokenStream:
            def read1(self, _):
                raise OSError('fixture exception payload must not escape')
        events = queue.Queue(maxsize=4)
        module['consume_output'](BrokenStream(), events, threading.Event())
        self.assertEqual(events.get_nowait(), ('error', 'OSError'))
        self.assertEqual(events.get_nowait(), ('eof', None))
        self.assertTrue(events.empty())

    def test_logging_supports_all_four_primary_levels(self):
        module = runpy.run_path(str(self.runner))
        logger = module['configure_logging'](self.project, debug=True)
        try:
            for level in ('INFO', 'WARNING', 'ERROR', 'DEBUG'):
                logger.log(getattr(logging, level), 'Safe fixture status.')
        finally:
            module['close_logging'](logger)
        logs = list((self.project / 'logs').glob('check_*.log'))
        self.assertEqual(len(logs), 1)
        text = logs[0].read_text(encoding='utf-8')
        for level in ('INFO', 'WARNING', 'ERROR', 'DEBUG'):
            self.assertIn(f'[{level}]', text)
        if os.name != 'nt':
            self.assertEqual(logs[0].stat().st_mode & 0o777, 0o600)

    def test_missing_executable_fails_without_traceback(self):
        result = self.invoke([str(self.fixture / 'missing-command')])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'[ERROR]', result.stderr)
        self.assertNotIn(b'Traceback', result.stderr)


if __name__ == '__main__':
    logging.basicConfig(level=logging.INFO, format='%(asctime)s [%(levelname)s] [check-tests] %(message)s')
    LOG.info('Running stream and process ownership regression suite.')
    unittest.main(verbosity=2)
