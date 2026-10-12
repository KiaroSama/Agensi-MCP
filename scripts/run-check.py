"""Bound and reap one CI check, including descendants surviving their direct parent."""
from __future__ import annotations

import argparse
import ctypes
from ctypes import wintypes
import datetime
import logging
import os
from pathlib import Path
import queue
import signal
import subprocess
import sys
import threading
import time
import uuid


class WindowsJob:
    def __init__(self):
        self.api = ctypes.WinDLL('kernel32', use_last_error=True)
        self.api.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
        self.api.CreateJobObjectW.restype = wintypes.HANDLE
        self.api.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
        self.api.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        self.api.QueryInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p]
        self.api.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
        self.api.CloseHandle.argtypes = [wintypes.HANDLE]
        self.handle = self.api.CreateJobObjectW(None, None)
        if not self.handle:
            raise OSError('Cannot create owned Windows job')
        # JOBOBJECT_EXTENDED_LIMIT_INFORMATION: basic limit flags at offset 16, native pointer layout.
        info = ctypes.create_string_buffer(144 if ctypes.sizeof(ctypes.c_void_p) == 8 else 112)
        ctypes.c_uint32.from_buffer(info, 16).value = 0x2000  # KILL_ON_JOB_CLOSE, no breakaway.
        if not self.api.SetInformationJobObject(self.handle, 9, info, len(info)):
            self.close()
            raise OSError('Cannot bound Windows job')

    def assign(self, process):
        if not self.api.AssignProcessToJobObject(self.handle, wintypes.HANDLE(int(process._handle))):
            raise OSError('Cannot assign gated check to Windows job')

    def active(self):
        info = ctypes.create_string_buffer(48)  # JOBOBJECT_BASIC_ACCOUNTING_INFORMATION.
        if not self.api.QueryInformationJobObject(self.handle, 1, info, len(info), None):
            raise OSError('Cannot verify owned Windows job')
        return ctypes.c_uint32.from_buffer(info, 40).value

    def stop(self):
        if not self.api.TerminateJobObject(self.handle, 1):
            raise OSError('Cannot terminate owned Windows job')
        deadline = time.monotonic() + 5
        while self.active():
            if time.monotonic() >= deadline:
                raise TimeoutError('Owned Windows job did not empty')
            time.sleep(0.02)

    def close(self):
        if self.handle:
            self.api.CloseHandle(self.handle)
            self.handle = None


def bootstrap():
    # The owner assigns this process to its job before allowing any command/descendant to start.
    if sys.stdin.buffer.readline() != b'start\n':
        return 1
    result = subprocess.run(sys.argv[2:], stdin=subprocess.DEVNULL, check=False)
    return result.returncode


def configure_logging(root):
    log = logging.getLogger('check')
    log.setLevel(logging.INFO)
    formatter = logging.Formatter('[%(asctime)s UTC] [%(levelname)s] [check] %(message)s', '%Y-%m-%d %H:%M:%S')
    formatter.converter = time.gmtime
    console = logging.StreamHandler(sys.stderr)
    console.setFormatter(formatter)
    log.addHandler(console)
    try:
        directory = root / 'logs'
        directory.mkdir(exist_ok=True)
        stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%d_%H-%M-%S_UTC')
        path = directory / f'check_{stamp}_{uuid.uuid4().hex}.log'
        with path.open('x', encoding='utf-8'):
            pass
        handler = logging.FileHandler(path, encoding='utf-8')
        handler.setFormatter(formatter)
        log.addHandler(handler)
    except OSError:
        log.warning('File logging unavailable; check status uses stderr.')
    return log


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--wall', type=float, required=True)
    parser.add_argument('--idle', type=float, required=True)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command or min(args.wall, args.idle) <= 0:
        parser.error('A command and positive wall/idle bounds are required')
    root = Path(__file__).resolve().parents[1]
    log = configure_logging(root)
    job = None
    assigned = False
    process = None
    readers = []
    result = 1
    started = time.monotonic()
    events = queue.Queue()
    try:
        if os.name == 'nt':
            job = WindowsJob()
            launched = [sys.executable, '-I', str(Path(__file__).resolve()), '--bootstrap', *command]
        else:
            launched = command
        process = subprocess.Popen(launched, cwd=root, stdin=subprocess.PIPE if job else subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8',
                                   start_new_session=not bool(job), creationflags=0x08000000 if job else 0)
        if job:
            job.assign(process)
            assigned = True
            process.stdin.buffer.write(b'start\n')
            process.stdin.close()
        log.info('Started pid=%s wall=%s idle=%s; arguments omitted.', process.pid, args.wall, args.idle)

        def consume(stream):
            try:
                for line in stream:
                    events.put(line)
            finally:
                events.put(None)

        for stream in (process.stdout, process.stderr):
            thread = threading.Thread(target=consume, args=(stream,))
            thread.start()
            readers.append(thread)
        complete = 0
        progress = started
        while complete < 2 or process.poll() is None:
            now = time.monotonic()
            if now - started > args.wall or now - progress > args.idle:
                raise TimeoutError('Check exceeded wall or idle limit')
            try:
                line = events.get(timeout=0.05)
            except queue.Empty:
                continue
            if line is None:
                complete += 1
            else:
                progress = time.monotonic()
                sys.stdout.write(line)
                sys.stdout.flush()
        result = process.wait(timeout=5)
        log.info('Completed exit=%s duration_ms=%s.', result, round((time.monotonic() - started) * 1000))
    except (OSError, ValueError, TimeoutError, subprocess.SubprocessError) as error:
        log.error('Check failed (%s); payload omitted.', type(error).__name__)
    finally:
        try:
            if job:
                if process and not assigned and process.poll() is None:
                    process.kill()  # The bootstrap gate has not permitted any descendant yet.
                job.stop()
            elif process:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            if process:
                process.wait(timeout=5)
            for thread in readers:
                thread.join(timeout=5)
                if thread.is_alive():
                    raise TimeoutError('Owned output reader did not stop')
        except (OSError, TimeoutError, subprocess.SubprocessError) as error:
            result = 1
            log.error('Owned cleanup failed (%s); no successful verification claimed.', type(error).__name__)
        finally:
            if job:
                job.close()
            if process:
                for stream in (process.stdin, process.stdout, process.stderr):
                    if stream and not stream.closed:
                        stream.close()
            logging.shutdown()
    return result


if __name__ == '__main__':
    raise SystemExit(bootstrap() if sys.argv[1:2] == ['--bootstrap'] else main())
