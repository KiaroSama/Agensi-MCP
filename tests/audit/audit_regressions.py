"""Run bounded black-box Agensi regressions against an explicitly supplied native binary.

All traffic terminates in a loopback fixture. Only synthetic credentials are used.
No user configuration is read; all project roots, sessions and CA keys are disposable.
"""
from __future__ import annotations
import argparse
import contextlib
import datetime
import hashlib
import http.client
import json
import logging
import os
from pathlib import Path
import queue
import shutil
import socket
import subprocess
import tempfile
import threading
import time
import urllib.parse
from mock_upstream import LocalFixture, make_certificates

LOG = logging.getLogger('audit')
RESULTS = []
SOURCE = Path(__file__).resolve().parents[2]
WORK = SOURCE / '.tmp' / 'audit-blackbox'


def record(name: str, passed: bool, observed: object, finding: str | None = None) -> None:
    """Record desired-contract assertions, not expected failures disguised as passes."""
    row = {'case': name, 'status': 'PASS' if passed else 'FAIL', 'observed': observed}
    if finding:
        row['finding'] = finding
    RESULTS.append(row)
    (LOG.info if passed else LOG.warning)('%s %s', row['status'], name)


class Project:
    def __init__(self):
        self.root = Path(tempfile.mkdtemp(prefix='agensi audit-\u00e9-', dir=WORK))
        (self.root / 'bin').mkdir()
        self.binary = self.root / 'bin/agensi-mcp'
        shutil.copy2(SOURCE / 'bin/agensi-mcp', self.binary)
        self.binary.chmod(0o700)
        for name in ('Cargo.toml', 'run.ps1'):
            shutil.copy2(SOURCE / name, self.root / name)
        self.env = os.environ.copy()
        self.env.update({'RUST_BACKTRACE': '0', 'TEMP': str(self.root / '.tmp'),
                         'TMP': str(self.root / '.tmp'), 'TMPDIR': str(self.root / '.tmp')})
        (self.root / '.tmp').mkdir()
        self.processes = []
        self.fixture = None
    def __enter__(self):
        return self
    def __exit__(self, *_):
        for process in self.processes:
            process.stop()
        if self.fixture:
            self.fixture.close()
        shutil.rmtree(self.root)
    def config(self, value=None):
        if value is None:
            value = {'schema': 1, 'mode': 'disconnected', 'selected': None, 'profiles': []}
        (self.root / 'config.local.json').write_text(json.dumps(value), encoding='utf-8')
    def route(self, protocol='http', user=False):
        if not self.fixture:
            ca, ctx = make_certificates(self.root / 'fixture-tls')
            self.fixture = LocalFixture(ctx).start()
            self.env.update({'SSL_CERT_FILE': str(ca), 'SSL_CERT_DIR': str(ca.parent)})
        self.config({'schema': 1, 'mode': 'proxy', 'selected': 'fixture',
                     'profiles': [{'name': 'fixture', 'url': f'{protocol}://'
                                  f'{"fixture-user@" if user else ""}127.0.0.1:{self.fixture.port}'}]})
    def run(self, *args, timeout=3, stdin=None):
        try:
            process = subprocess.run([str(self.binary), *map(str, args)], env=self.env,
                                     cwd='/', input=stdin, text=True, capture_output=True, timeout=timeout)
            try:
                result = json.loads(process.stdout)
            except (ValueError, TypeError):
                result = None
            return {'exit': process.returncode, 'json': result, 'stderr': process.stderr,
                    'stdout': process.stdout, 'timeout': False}
        except subprocess.TimeoutExpired:
            return {'exit': None, 'timeout': True}
    def start(self, action):
        process = NativeProcess(self, action)
        self.processes.append(process)
        return process
    def signin(self, protocol='http', user=False):
        self.route(protocol, user)
        process = self.start('signin')
        if not process.ready.wait(3):
            return process, False
        status = process.callback()
        try:
            code = process.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            code = None
        return process, status == 200 and code == 0 and self.session_exists()
    def session_exists(self):
        return (self.root / '.state/auth/rust-session.json').is_file()


class NativeProcess:
    def __init__(self, project, action):
        self.project = project
        self.ready = threading.Event()
        self.http_ready = threading.Event()
        self.query = None
        self.stderr_lines = []
        self.process = subprocess.Popen([str(project.binary), action], env=project.env, cwd='/',
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, text=True)
        self.reader = threading.Thread(target=self.consume, daemon=True)
        self.reader.start()
    def consume(self):
        for line in self.process.stderr:
            if line.startswith('https://auth.agensi-audit.test/authorize?'):
                self.query = urllib.parse.parse_qs(urllib.parse.urlsplit(line.strip()).query)
                self.ready.set()
                continue
            if 'Local MCP HTTP listening' in line:
                self.http_ready.set()
            self.stderr_lines.append(line)
    def path(self, extra='', wrong_state=False):
        assert self.query is not None, 'Synthetic authorization not ready'
        target = urllib.parse.urlsplit(self.query['redirect_uri'][0])
        parameters = {'code': 'synthetic-fixture-code',
                      'state': 'wrong-state' if wrong_state else self.query['state'][0],
                      'iss': self.project.fixture.issuer}
        return target.port, target.path + '?' + urllib.parse.urlencode(parameters) + extra
    def callback(self, extra='', wrong_state=False, fragment=False, host=None, headers=None):
        port, path = self.path(extra, wrong_state)
        header = f'Host: {host or f"127.0.0.1:{port}"}\r\n' if headers is None else headers.format(port=port)
        request = f'GET {path} HTTP/1.1\r\n{header}\r\n'.encode()
        try:
            with socket.create_connection(('127.0.0.1', port), timeout=2) as connection:
                if fragment:
                    connection.sendall(b'GET /oauth/call')
                    time.sleep(0.15)
                    connection.sendall(request[len(b'GET /oauth/call'):])
                else:
                    connection.sendall(request)
                answer = connection.recv(4096)
                return int(answer.split(b' ', 2)[1]) if answer.startswith(b'HTTP/') else None
        except (OSError, ValueError):
            return None
    def stop(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=3)
        self.reader.join(timeout=2)
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if stream:
                stream.close()


def management_cases():
    oracle = json.loads((SOURCE / 'tests/fixtures/migration.json').read_text())
    for group in ('proxies', 'configs', 'lists'):
        for case in oracle[group]:
            with Project() as p:
                if group == 'proxies':
                    result = p.run('add', 'fixture', '--stdin', stdin=case['input'])
                    expected = (case['expected'] or {}).get('status')
                elif group == 'configs':
                    p.config(case['input'])
                    result = p.run('status')
                    expected = case['expected']
                else:
                    source = p.root / 'synthetic-proxies.txt'
                    source.write_text(case['input'], encoding='utf-8')
                    result = p.run('import', source)
                    expected = case['expected']
                error = case['error']
                if group == 'proxies' and case['name'] in ('proxy-20', 'proxy-21'):
                    # CLI deliberately trims input; the lower-level oracle does not.
                    error = None
                    expected = {'mode': 'disconnected', 'selected': None, 'profiles': [{'name': 'fixture', 'protocol': 'http', 'host': 'localhost', 'port': 80, 'authenticated': False}]}
                passed = not result['timeout'] and ((result['exit'] != 0 and
                    error in result.get('stderr', '')) if error else result['exit'] == 0)
                if passed and not error and group != 'lists':
                    passed = result['json'] == expected
                if passed and not error and group == 'lists':
                    saved = json.loads((p.root / 'config.local.json').read_text())
                    unique = {}
                    for entry in expected:
                        unique.setdefault(entry['name'].lower(), {'name': entry['name'], 'url': entry['url']})
                    passed = saved['profiles'] == list(unique.values())
                record('oracle-' + case['name'], passed, {'exit': result['exit'], 'timeout': result['timeout']})
    for index, filename in enumerate(('SKILL.md', 'skill.md', 'Skill.md')):
        with Project() as p:
            source = p.root / 'synthetic-skill.json'
            source.write_text(json.dumps([{'path': filename, 'content': '# Synthetic fixture'}]))
            result = p.run('save-skill', 'case-' + str(index), source)
            target = p.root / 'skills' / ('case-' + str(index))
            # Accepted packages must contain the exact entry point on a case-sensitive filesystem.
            valid = result['exit'] == 0 and (target / 'SKILL.md').is_file()
            record('skill-entry-' + filename, valid,
                   {'exit': result['exit'], 'files': sorted(v.name for v in target.iterdir()) if target.exists() else []}, 'F04')
    with Project() as p:
        p.config({'schema': 1, 'mode': 'disconnected', 'selected': None, 'profiles': [],
                  'future': {'keep': ['unicode-\u00e9', 17]}})
        result = p.run('direct')
        saved = json.loads((p.root / 'config.local.json').read_text())
        record('unknown-config-preserved', result['exit'] == 0 and saved['future'] == {'keep': ['unicode-\u00e9', 17]}, {'exit': result['exit']})
        before = (p.root / 'config.local.json').read_bytes()
        result = p.run('select', 'missing')
        record('failed-config-update-preserves-bytes', result['exit'] != 0 and before == (p.root / 'config.local.json').read_bytes(), {'exit': result['exit']})
        record('disconnected-network-check-refuses', p.run('disconnect')['exit'] == 0 and p.run('network-check')['exit'] != 0, {})
    for path in ('../outside', '/absolute', 'C:\\escape', 'NUL.txt', 'nested/../escape'):
        with Project() as p:
            source = p.root / 'synthetic-skill.json'
            source.write_text(json.dumps([{'path': 'SKILL.md', 'content': '# Fixture'}, {'path': path, 'content': 'fixture'}]))
            result = p.run('save-skill', 'bad', source)
            record('skill-refuses-' + path, result['exit'] != 0 and not (p.root / 'skills/bad').exists(), {'exit': result['exit']})


def filesystem_cases():
    for kind in ('config', 'session', 'import', 'skill-input'):
        with Project() as p:
            p.config()
            paths = {'config': p.root / 'config.local.json', 'session': p.root / '.state/auth/rust-session.json',
                     'import': p.root / 'fifo-import', 'skill-input': p.root / 'fifo-skill'}
            path = paths[kind]
            path.parent.mkdir(parents=True, exist_ok=True)
            path.unlink(missing_ok=True)
            os.mkfifo(path)
            args = {'config': ['status'], 'session': ['doctor'], 'import': ['import', str(path)],
                    'skill-input': ['save-skill', 'fixture', str(path)]}[kind]
            result = p.run(*args, timeout=0.9)
            # Doctor reports an unreadable session as diagnostic JSON, not a CLI failure.
            refused = result['exit'] != 0
            if kind == 'session':
                session = (result.get('json') or {}).get('session', {})
                refused = (result['exit'] == 0 and session.get('readable') is False
                           and session.get('usable') == 'unknown')
            record('fifo-' + kind + '-bounded', not result['timeout'] and refused,
                   {'timeout': result['timeout'], 'bound_seconds': 0.9,
                    'refused': refused}, 'F05')
    with Project() as p:
        (p.root / 'logs').write_text('synthetic obstruction')
        result = p.run('status')
        text = result.get('stderr', '')
        record('stderr-logging-fallback-events', result['exit'] == 0 and '[INFO]' in text and 'startup' in text and 'shutdown' in text,
               {'exit': result['exit'], 'timestamped_info_present': '[INFO]' in text}, 'F07')
    with Project() as p:
        p.config()
        outside = p.root / 'outside-synthetic-state'
        outside.mkdir()
        (p.root / '.state').symlink_to(outside, target_is_directory=True)
        result = p.run('status')
        record('status-does-not-touch-unused-state-link', result['exit'] == 0 and not list(outside.iterdir()), {'exit': result['exit']})
    with Project() as p:
        p.route()
        # The normal runtime guard creates .state, but does not protect its auth child.
        (p.root / '.state').mkdir(exist_ok=True)
        outside = p.root / 'outside-synthetic-auth'
        outside.mkdir()
        (p.root / '.state/auth').symlink_to(outside, target_is_directory=True)
        process = p.start('signin')
        process.ready.wait(1)
        process.stop()
        children = sorted(v.name for v in outside.iterdir())
        record('linked-auth-directory-never-written', not children, {'outside_names': children}, 'F05')
    with Project() as p:
        p.config()
        (p.root / '.state/auth').mkdir(parents=True)
        outside = p.root / 'outside-session.json'
        outside.write_text(json.dumps({'client_id': 'synthetic-client', 'token_response': None,
                                       'requested_scopes': [], 'token_received_at': None, 'issuer': None}))
        (p.root / '.state/auth/rust-session.json').symlink_to(outside)
        result = p.run('doctor')
        session = (result.get('json') or {}).get('session', {})
        record('linked-session-not-loaded', session.get('readable') is not True,
               {'exit': result['exit'], 'readable': session.get('readable')}, 'F05')


def oauth_cases():
    for protocol, user in [('http', False), ('socks4', False), ('socks4a', False), ('socks5', False), ('socks5h', False), ('socks4', True), ('socks4a', True)]:
        with Project() as p:
            process, success = p.signin(protocol, user)
            record(f'oauth-{protocol}-userid-{user}', success,
                   {'exit': process.process.poll(), 'session_saved': p.session_exists(),
                    'proxy_connections': len(p.fixture.connections)}, 'F01' if user else None)
    for mutation in ('fragment', 'invalid-utf8', 'idle', 'wrong-state', 'duplicate-state', 'duplicate-code', 'wrong-host', 'missing-host', 'duplicate-host', 'mixed-case-host'):
        with Project() as p:
            p.route()
            process = p.start('signin')
            if not process.ready.wait(3):
                record('oauth-callback-' + mutation, False, {'fixture_ready': False})
                continue
            status = None
            if mutation in ('invalid-utf8', 'idle'):
                port, _ = process.path()
                with socket.create_connection(('127.0.0.1', port), timeout=2) as connection:
                    if mutation == 'idle':
                        time.sleep(5.3)
                    else:
                        connection.sendall(b'GET /oauth/callback?x=\xff HTTP/1.1\r\n\r\n')
                        time.sleep(0.15)
                alive = process.process.poll() is None
                status = process.callback() if alive else None
                passed = alive and status == 200
            elif mutation == 'fragment':
                status = process.callback(fragment=True)
                passed = status == 200
            elif mutation == 'wrong-state':
                status = process.callback(wrong_state=True)
                passed = status == 400 and process.callback() == 200
            elif mutation == 'mixed-case-host':
                status = process.callback(headers='hOsT: 127.0.0.1:{port}\r\n')
                passed = status == 200
            elif mutation in ('wrong-host', 'missing-host', 'duplicate-host'):
                if mutation == 'wrong-host':
                    status = process.callback(host='evil.invalid')
                else:
                    header = '' if mutation == 'missing-host' else 'Host: 127.0.0.1:{port}\r\nHost: 127.0.0.1:{port}\r\n'
                    status = process.callback(headers=header)
                passed = status == 400 and not p.session_exists()
                passed = passed and process.callback() == 200
            else:
                extra = '&state=other' if mutation == 'duplicate-state' else '&code=other'
                status = process.callback(extra=extra)
                passed = status == 400 and not p.session_exists()
            time.sleep(0.05)
            record('oauth-callback-' + mutation, passed,
                   {'callback_status': status, 'process_alive': process.process.poll() is None,
                    'session_saved': p.session_exists()}, 'F02')
    with Project() as p:
        p.route()
        process = p.start('signin')
        ready = process.ready.wait(3)
        p.run('disconnect')
        time.sleep(0.55)
        record('oauth-pending-revoked-on-disconnect', ready and process.process.poll() is not None,
               {'still_waiting_after_550ms': process.process.poll() is None}, 'F03')


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def http_request(port, method='POST', path='/mcp', headers=None, body=None):
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=3)
    try:
        selected_headers = {'Content-Type': 'application/json', 'Accept': 'application/json, text/event-stream'}
        selected_headers.update(headers or {})
        data = json.dumps({'jsonrpc': '2.0', 'id': 7, 'method': 'tools/list', 'params': {}}) if body is None else body
        connection.request(method, path, body=data, headers=selected_headers)
        response = connection.getresponse()
        return response.status, response.read()
    finally:
        connection.close()


def http_cases():
    with Project() as p:
        _, success = p.signin()
        if not success:
            record('http-fixture-signin', False, {})
            return
        port = free_port()
        result = p.run('http-port', port)
        process = p.start('http')
        ready = process.http_ready.wait(3)
        record('http-startup-from-stored-session', ready and result['exit'] == 0, {'ready': ready})
        if not ready:
            return
        for name, method, path, headers, expected in [
            ('ordinary-post', 'POST', '/mcp', {}, 200),
            ('foreign-origin', 'POST', '/mcp', {'Origin': 'https://evil.invalid'}, 403),
            ('null-origin', 'POST', '/mcp', {'Origin': 'null'}, 403),
            ('wrong-host', 'POST', '/mcp', {'Host': 'evil.invalid'}, 403),
            ('query-refused', 'POST', '/mcp?target=elsewhere', {}, 400),
            ('method-refused', 'PUT', '/mcp', {}, 405),
            ('wrong-path', 'POST', '/other', {}, 404),
            ('same-origin', 'POST', '/mcp', {'Origin': f'http://127.0.0.1:{port}'}, 200),
        ]:
            before = len(p.fixture.requests)
            status, _ = http_request(port, method, path, headers)
            forwarded = len(p.fixture.requests) - before
            record('local-http-' + name, status == expected and (expected == 200 or forwarded == 0),
                   {'status': status, 'forwarded_requests': forwarded})
        status, _ = http_request(port, body='x' * (2 * 1024 * 1024 + 1))
        record('local-http-request-size-limit', status == 413, {'status': status})
        p.run('disconnect')
        try:
            process.process.wait(timeout=1)
        except subprocess.TimeoutExpired:
            pass
        record('local-http-idle-disconnect', process.process.poll() is not None, {'exit': process.process.poll()})
    # Regression: pause initialize, disconnect, and ensure no later probe calls begin.
    with Project() as p:
        _, success = p.signin()
        p.fixture.pause_path = '/mcp'
        p.fixture.pause_rpc = 'initialize'
        process = p.start('probe')
        paused = p.fixture.paused.wait(3)
        before = len(p.fixture.requests)
        p.run('disconnect')
        time.sleep(0.35)
        p.fixture.resume.set()
        try:
            process.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            pass
        later = p.fixture.requests[before:]
        record('probe-no-requests-after-disconnect', paused and not later,
               {'fixture_paused': paused, 'post_disconnect_request_count': len(later),
                'post_disconnect_methods': [v.get('rpc_method') for v in later], 'exit': process.process.poll()}, 'F03')
    # During stored-session restoration, settings must never bind old route to a new snapshot.
    with Project() as p:
        _, success = p.signin()
        port = free_port()
        p.run('http-port', port)
        p.fixture.pause_path = '/.well-known/oauth-authorization-server'
        process = p.start('http')
        paused = p.fixture.paused.wait(1.5)
        if paused:
            p.run('disconnect')
            p.fixture.resume.set()
            time.sleep(0.5)
            forwarded = 0
            status = None
            if process.http_ready.is_set():
                before = len(p.fixture.requests)
                status, _ = http_request(port)
                forwarded = len(p.fixture.requests) - before
            record('http-startup-route-snapshot-consistent', not process.http_ready.is_set() and forwarded == 0,
                   {'fixture_paused': True, 'listener_started': process.http_ready.is_set(),
                    'request_status': status, 'forwarded_after_disconnect': forwarded}, 'F03')
        else:
            record('http-startup-race-fixture-reached', False,
                   {'note': 'No startup network request; this fixture cannot force the source-level race.'})


def main():
    global SOURCE, WORK
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-root', type=Path, default=SOURCE)
    parser.add_argument('--work-root', type=Path, default=WORK)
    parser.add_argument('--phase', choices=['management', 'filesystem', 'oauth', 'http', 'all'], default='all')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--debug', action='store_true')
    args = parser.parse_args()
    logging.basicConfig(level=logging.DEBUG if args.debug else logging.INFO,
                        format='%(asctime)s [%(levelname)s] %(message)s')
    SOURCE = args.source_root.resolve()
    WORK = args.work_root.resolve()
    WORK.mkdir(parents=True, exist_ok=True)
    if not (SOURCE / 'bin/agensi-mcp').is_file():
        parser.error('Expected a Linux native executable under source-root/bin/agensi-mcp')
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    groups = {'management': management_cases, 'filesystem': filesystem_cases,
              'oauth': oauth_cases, 'http': http_cases}
    try:
        for name, function in groups.items():
            if args.phase in ('all', name):
                LOG.info('Starting %s cases with isolated synthetic fixtures.', name)
                function()
    except Exception as error:
        # Exception payloads could include a fixture URL/state; retain only its class.
        LOG.error('Harness aborted with %s; payload omitted.', type(error).__name__)
        record('harness-completion', False, {'exception_type': type(error).__name__})
    if args.phase == 'all' and len(RESULTS) != 98:
        record('complete-inventory', False, {'expected': 98, 'observed': len(RESULTS)})
    report = {'started_utc': started, 'binary_sha256': hashlib.sha256((SOURCE / 'bin/agensi-mcp').read_bytes()).hexdigest(),
              'phase': args.phase, 'platform': 'Linux', 'synthetic_loopback_only': True,
              'cases': RESULTS, 'passed': sum(r['status'] == 'PASS' for r in RESULTS),
              'failed': sum(r['status'] == 'FAIL' for r in RESULTS)}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    LOG.info('Results written: passed=%s failed=%s', report['passed'], report['failed'])
    return 1 if report['failed'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
