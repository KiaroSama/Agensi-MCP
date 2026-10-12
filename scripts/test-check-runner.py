"""Check exit propagation and cleanup of a grandchild whose parent already exited."""
import ctypes
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def alive(pid):
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
        # Linux may temporarily retain a killed orphan as a non-running zombie.
        state = Path(f'/proc/{pid}/stat')
        if state.exists() and state.read_text(encoding='utf-8').split(') ', 1)[1].startswith('Z'):
            return False
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def main():
    base = ROOT / '.tmp'
    base.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='check-owner-', dir=base) as value:
        directory = Path(value)
        parent = directory / 'parent.py'
        pid_file = directory / 'grandchild.pid'
        lease = directory / 'fixture.lease'
        lease.touch()
        child = directory / 'child.py'
        child.write_text("from pathlib import Path\nimport sys,time\n"
                         "deadline=time.monotonic()+10\n"
                         "while Path(sys.argv[1]).exists() and time.monotonic()<deadline: time.sleep(0.02)\n", encoding='utf-8')
        parent.write_text("import subprocess,sys\nfrom pathlib import Path\n"
                          "child=subprocess.Popen([sys.executable,'-I',sys.argv[2],sys.argv[3]],"
                          "stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)\n"
                          "Path(sys.argv[1]).write_text(str(child.pid),encoding='utf-8')\n", encoding='utf-8')
        command = [sys.executable, '-I', str(ROOT / 'scripts/run-check.py'), '--wall', '5', '--idle', '2', '--']
        pid = None
        try:
            result = subprocess.run([*command, sys.executable, '-I', str(parent), str(pid_file), str(child), str(lease)],
                                    capture_output=True, timeout=12, encoding='utf-8')
            pid = int(pid_file.read_text(encoding='utf-8'))
            deadline = time.monotonic() + 3
            while alive(pid) and time.monotonic() < deadline:
                time.sleep(0.02)
            assert not alive(pid), 'Grandchild survived completed check'
            assert result.returncode == 0, 'Normal owned check failed'
        finally:
            # Failure-only cooperative cleanup uses the unique fixture lease, not a recycled PID.
            lease.unlink(missing_ok=True)
            if pid is None and pid_file.exists():
                pid = int(pid_file.read_text(encoding='utf-8'))
            deadline = time.monotonic() + 3
            while pid and alive(pid) and time.monotonic() < deadline:
                time.sleep(0.02)
            assert not pid or not alive(pid), 'Fixture cleanup did not complete'
        result = subprocess.run([*command, sys.executable, '-c', 'raise SystemExit(7)'],
                                capture_output=True, timeout=12, encoding='utf-8')
        assert result.returncode == 7, 'Exit code was not preserved'
        print('Bounded runner: exited-parent descendant cleanup and exit propagation passed.')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
