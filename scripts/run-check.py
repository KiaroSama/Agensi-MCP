"""Bound and reap one CI check, including descendants surviving their direct parent."""
from __future__ import annotations

import argparse
import ctypes
from ctypes import wintypes
import datetime
import logging
import math
import os
from pathlib import Path
import queue
import signal
import stat
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
    try:
        result = subprocess.run(sys.argv[2:], stdin=subprocess.DEVNULL, check=False)
        return result.returncode
    except (OSError, subprocess.SubprocessError) as error:
        # Bootstrap errors belong to the runner, not the untrusted child's output.
        stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%d %H:%M:%S')
        print(f'[{stamp} UTC] [ERROR] [check] Command launch failed ({type(error).__name__}); payload omitted.', file=sys.stderr)
        return 1


# Fixed-size chunks and a bounded queue prevent newline-free output from growing memory.
CHUNK_BYTES = 16 * 1024
PENDING_CHUNKS = 64


class OwnedLogHandler(logging.StreamHandler):
    """Own the exclusive file descriptor instead of reopening the path."""
    def emit(self, record):
        if self.stream is None:
            return
        try:
            self.stream.write(self.format(record) + self.terminator)
            self.flush()
        except (OSError, ValueError):
            # The console handler already received this safe status record.
            self.close()

    def close(self):
        self.acquire()
        try:
            stream, self.stream = self.stream, None
            if stream:
                try:
                    stream.close()
                except OSError:
                    pass
        finally:
            self.release()
            super().close()


def close_logging(log):
    for handler in list(log.handlers):
        log.removeHandler(handler)
        handler.close()


def reject_linked_path(path):
    # Cooperative local-path checks, not a hostile filesystem sandbox.
    for part in (path, *path.parents):
        try:
            metadata = part.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISLNK(metadata.st_mode) or getattr(metadata, 'st_file_attributes', 0) & 0x400:
            raise OSError('Linked log path refused')


def configure_logging(root, debug=False):
    log = logging.getLogger('check')
    close_logging(log)
    log.propagate = False
    log.setLevel(logging.DEBUG if debug else logging.INFO)
    formatter = logging.Formatter('[%(asctime)s UTC] [%(levelname)s] [check] %(message)s', '%Y-%m-%d %H:%M:%S')
    formatter.converter = time.gmtime
    console = logging.StreamHandler(sys.stderr)
    console.setFormatter(formatter)
    log.addHandler(console)
    try:
        directory = root / 'logs'
        reject_linked_path(directory)
        directory.mkdir(mode=0o700, exist_ok=True)
        reject_linked_path(directory)
        stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%d_%H-%M-%S_UTC')
        path = directory / f'check_{stamp}_{uuid.uuid4().hex}.log'
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, 'O_NOFOLLOW', 0)
        descriptor = os.open(path, flags, 0o600)
        stream = os.fdopen(descriptor, 'w', encoding='utf-8')
        handler = OwnedLogHandler(stream)
        handler.setFormatter(formatter)
        log.addHandler(handler)
    except OSError:
        log.warning('File logging unavailable; check status uses stderr.')
    return log


def positive_finite(value):
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError('A finite positive bound is required')
    return number


def enqueue(events, stop, event):
    while not stop.is_set():
        try:
            events.put(event, timeout=0.05)
            return True
        except queue.Full:
            pass
    return False


def consume_output(stream, events, stop):
    try:
        while not stop.is_set():
            chunk = stream.read1(CHUNK_BYTES)
            if not chunk:
                break
            if not enqueue(events, stop, ('chunk', chunk)):
                return
    except Exception as error:
        # A reader failure is not EOF; carry only the type, never the exception payload.
        enqueue(events, stop, ('error', type(error).__name__))
    finally:
        enqueue(events, stop, ('eof', None))


def cleanup_owned(process, job, assigned, readers, stop, log):
    stop.set()  # Release readers waiting on a full queue before joining them.
    success = True
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
    except (OSError, TimeoutError, subprocess.SubprocessError) as error:
        success = False
        log.error('Owned termination failed (%s); payload omitted.', type(error).__name__)
    finally:
        if job:
            job.close()  # KILL_ON_JOB_CLOSE remains a backstop even when verification fails.
    if process:
        try:
            process.wait(timeout=5)
        except (OSError, TimeoutError, subprocess.SubprocessError) as error:
            success = False
            log.error('Owned wait failed (%s); payload omitted.', type(error).__name__)
    deadline = time.monotonic() + 5
    for thread, stream in readers:
        thread.join(timeout=max(0, deadline - time.monotonic()))
        if thread.is_alive():
            success = False
            log.error('Owned output reader did not stop; no successful verification claimed.')
        else:
            stream.close()
    if process and process.stdin and not process.stdin.closed:
        process.stdin.close()
    return success


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--wall', type=positive_finite, required=True)
    parser.add_argument('--idle', type=positive_finite, required=True)
    parser.add_argument('--debug', action='store_true')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('A command is required')
    root = Path(__file__).resolve().parents[1]
    log = configure_logging(root, args.debug)
    job = None
    assigned = False
    process = None
    readers = []
    stop = threading.Event()
    result = 1
    started = time.monotonic()
    events = queue.Queue(maxsize=PENDING_CHUNKS)
    try:
        if os.name == 'nt':
            job = WindowsJob()
            launched = [sys.executable, '-I', str(Path(__file__).resolve()), '--bootstrap', *command]
        else:
            launched = command
        process = subprocess.Popen(launched, cwd=root, stdin=subprocess.PIPE if job else subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   start_new_session=not bool(job), creationflags=0x08000000 if job else 0)
        if job:
            job.assign(process)
            assigned = True
            process.stdin.write(b'start\n')
            process.stdin.close()
        log.info('Started pid=%s wall=%s idle=%s; arguments omitted.', process.pid, args.wall, args.idle)
        log.debug('Output queue bounded to %s chunks of at most %s bytes.', PENDING_CHUNKS, CHUNK_BYTES)
        for stream in (process.stdout, process.stderr):
            thread = threading.Thread(target=consume_output, args=(stream, events, stop), daemon=True)
            thread.start()
            readers.append((thread, stream))
        complete = 0
        progress = started
        while complete < 2 or process.poll() is None:
            now = time.monotonic()
            if now - started > args.wall or now - progress > args.idle:
                raise TimeoutError('Check exceeded wall or idle limit')
            try:
                kind, value = events.get(timeout=0.05)
            except queue.Empty:
                continue
            if kind == 'eof':
                complete += 1
            elif kind == 'error':
                log.error('Output reader failed (%s); payload omitted.', value)
                raise OSError('Cannot verify complete command output')
            else:
                progress = time.monotonic()
                # Preserve arbitrary bytes and split UTF-8 sequences; never decode in reader threads.
                sys.stdout.buffer.write(value)
                sys.stdout.buffer.flush()
        result = process.wait(timeout=5)
    except (OSError, ValueError, TimeoutError, subprocess.SubprocessError) as error:
        result = 1
        log.error('Check failed (%s); payload omitted.', type(error).__name__)
    except KeyboardInterrupt:
        result = 130
        log.warning('Check interrupted; stopping owned processes.')
    finally:
        try:
            if not cleanup_owned(process, job, assigned, readers, stop, log):
                result = 1
        except (OSError, ValueError, TimeoutError, subprocess.SubprocessError) as error:
            result = 1
            log.error('Owned cleanup failed (%s); no successful verification claimed.', type(error).__name__)
        log.log(logging.INFO if result == 0 else logging.ERROR, 'Completed exit=%s duration_ms=%s.', result, round((time.monotonic() - started) * 1000))
        close_logging(log)
    return result


if __name__ == '__main__':
    raise SystemExit(bootstrap() if sys.argv[1:2] == ['--bootstrap'] else main())
