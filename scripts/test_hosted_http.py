#!/usr/bin/env python3
"""Hosted HTTP/HTTPS checks against independent Python peers, at O0 and O3.

No public network or checked-in credentials. --skip-tls is explicit, never an
implicit pass. TLS builds require the same OpenSSL >=3.5 setup as tls_library.
"""
import argparse
import errno
import http.client
import json
import os
from pathlib import Path
import socket
import ssl
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]


def run(command, **kwargs):
    result = subprocess.run(list(map(str, command)), capture_output=True, timeout=120, **kwargs)
    if result.returncode:
        raise RuntimeError(f"{command}: {result.returncode}\n{result.stdout.decode(errors='replace')}\n{result.stderr.decode(errors='replace')}")
    return result


def credentials(directory):
    # DNS-only leaf also exercises mandatory IP SAN verification failures.
    (directory / 'extensions.cnf').write_text(
        'subjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\n')
    for args in [
        ['req', '-x509', '-newkey', 'rsa:2048', '-noenc', '-keyout', 'ca.key', '-out', 'ca.pem', '-subj', '/CN=Dodo hosted local CA', '-days', '2', '-addext', 'basicConstraints=critical,CA:TRUE', '-addext', 'keyUsage=critical,keyCertSign,cRLSign'],
        ['req', '-new', '-newkey', 'rsa:2048', '-noenc', '-keyout', 'key.pem', '-out', 'leaf.csr', '-subj', '/CN=localhost'],
        ['x509', '-req', '-in', 'leaf.csr', '-CA', 'ca.pem', '-CAkey', 'ca.key', '-CAcreateserial', '-out', 'cert.pem', '-days', '1', '-extfile', 'extensions.cnf'],
    ]:
        run(['openssl', *args], cwd=directory)
    os.chmod(directory / 'key.pem', 0o600)
    os.chmod(directory / 'ca.key', 0o600)


class Peer:
    def __init__(self, directory=None):
        self.listener = socket.socket()
        self.listener.bind(('127.0.0.1', 0))
        self.listener.listen(32)
        self.listener.settimeout(.1)
        self.port = self.listener.getsockname()[1]
        self.context = None
        if directory:
            self.context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            self.context.load_cert_chain(directory / 'cert.pem', directory / 'key.pem')
            self.context.set_alpn_protocols(['http/1.1'])
        self.errors = []
        self.requests = []
        self.closed = 0
        self.pid = None
        self.fd_max = 0
        self.stop = threading.Event()
        self.workers = []
        self.thread = threading.Thread(target=self.accept, daemon=True)
        self.thread.start()

    def accept(self):
        while not self.stop.is_set():
            try:
                connection, _ = self.listener.accept()
            except TimeoutError:
                continue
            except OSError:
                break
            worker = threading.Thread(target=self.respond, args=(connection,), daemon=True)
            self.workers.append(worker)
            worker.start()

    def respond(self, connection):
        try:
            with connection:
                connection.settimeout(3)
                if self.context:
                    try:
                        connection = self.context.wrap_socket(connection, server_side=True)
                    except ssl.SSLError:
                        return  # Expected client verification failures.
                with connection:
                    incoming = b''
                    while b'\r\n\r\n' not in incoming:
                        part = connection.recv(1024)
                        if not part:
                            raise RuntimeError('EOF before client request head')
                        incoming += part
                        assert len(incoming) <= 16384
                    head, body = incoming.split(b'\r\n\r\n', 1)
                    lines = head.split(b'\r\n')
                    method, target, version = lines[0].split(b' ')
                    fields = [line.split(b':', 1) for line in lines[1:]]
                    headers = {name.lower(): value.strip() for name, value in fields}
                    assert version == b'HTTP/1.1'
                    assert headers[b'host'] == f'localhost:{self.port}'.encode()
                    assert sum(name.lower() == b'host' for name, _ in fields) == 1
                    assert b'transfer-encoding' not in headers and headers[b'connection'] == b'close'
                    length = int(headers[b'content-length'])
                    while len(body) < length:
                        body += connection.recv(1024)
                    assert len(body) == length
                    path = target.split(b'?', 1)[0]
                    self.requests.append((method, path, body))
                    if self.pid and Path(f'/proc/{self.pid}/fd').exists():
                        self.fd_max = max(self.fd_max, len(list(Path(f'/proc/{self.pid}/fd').iterdir())))
                    if path == b'/fragmented':
                        message = (b'HTTP/1.1 103 Early Hints\r\nX-Ignored: yes\r\n\r\nHTTP/1.1 200 OK\r\n'
                                   b'Transfer-Encoding: chunked\r\nX-Test: first\r\nX-Test: second\r\n\r\n'
                                   b'7\r\nHello, \r\n6\r\nDodo!\n\r\n0\r\nX-Trailer: yes\r\n\r\n')
                        for offset in range(0, len(message), 3):
                            connection.sendall(message[offset:offset + 3])
                            time.sleep(.0005)
                    elif path in (b'/redirect307', b'/redirect303'):
                        code = b'307 Temporary Redirect' if path.endswith(b'307') else b'303 See Other'
                        connection.sendall(b'HTTP/1.1 ' + code + b'\r\nLocation: /echo\r\nContent-Length: 0\r\n\r\n')
                    elif path == b'/echo':
                        assert headers[b'x-custom'] == b'kept'
                        assert (method, body) in [(b'POST', b'repeat me'), (b'GET', b'')]
                        assert (b'content-type' in headers) == (method == b'POST')
                        connection.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: ' + str(len(body)).encode() + b'\r\n\r\n' + body)
                    elif path == b'/head':
                        assert method == b'HEAD'
                        connection.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: 19\r\n\r\n')
                    elif path == b'/close':
                        connection.sendall(b'HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nhello')
                        connection.shutdown(socket.SHUT_WR)
                    elif path == b'/chunklimit':
                        connection.sendall(b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n9\r\n123456789\r\n0\r\n\r\n')
                    elif path == b'/badchunk':
                        connection.sendall(b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\ng\r\nx\r\n')
                    elif path == b'/badtrailer':
                        connection.sendall(b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nContent-Length: 0\r\n\r\n')
                    elif path == b'/large':
                        connection.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n123456789')
                    elif path == b'/headers':
                        connection.sendall(b'HTTP/1.1 200 OK\r\nX-Large: ' + b'a' * 600 + b'\r\n\r\n')
                    elif path == b'/malformed':
                        connection.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nx')
                    elif path == b'/truncated':
                        assert self.context
                        connection.sendall(b'HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nabc')
                        os.close(connection.detach())  # Deliberately omit TLS close_notify.
                        self.closed += 1
                        return
                    elif path == b'/eof':
                        connection.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhi')
                        connection.shutdown(socket.SHUT_WR)
                    elif path == b'/timeout':
                        time.sleep(.4)
                    elif path == b'/cross':
                        connection.sendall(b'HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/secret\r\nContent-Length: 0\r\n\r\n')
                    elif path == b'/loop':
                        connection.sendall(b'HTTP/1.1 302 Found\r\nLocation: /loop\r\nContent-Length: 0\r\n\r\n')
                    else:
                        assert path in (b'/fast', b'/'), path
                        connection.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello')
                    try:
                        # Successful HTTP and all failures must release the socket.
                        assert connection.recv(1) == b''
                    except (ConnectionResetError, ssl.SSLError):
                        pass
                    self.closed += 1
        except Exception as error:
            self.errors.append(error)

    def execute(self, executable):
        process = subprocess.Popen([str(executable)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.pid = process.pid
        try:
            stdout, stderr = process.communicate(timeout=30)
            assert process.returncode == 0, (process.returncode, stdout, stderr)
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate()
        for worker in self.workers:
            worker.join(5)
        assert not self.errors, self.errors
        assert self.closed == len(self.requests), (self.closed, len(self.requests))
        assert self.fd_max <= 8, f'client descriptor leak: {self.fd_max}'

    def close(self):
        self.stop.set()
        self.listener.close()
        self.thread.join(5)
        for worker in self.workers:
            worker.join(5)


def available_port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def connect(port, process):
    until = time.monotonic() + 10
    while time.monotonic() < until:
        if process.poll() is not None:
            raise RuntimeError(f'server exited {process.communicate()}')
        try:
            return socket.create_connection(('127.0.0.1', port), timeout=2)
        except ConnectionRefusedError:
            time.sleep(.01)
    raise RuntimeError('server did not start')


def server_checks(executable, port, context=None, cancellation=False):
    process = subprocess.Popen([str(executable)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        cases = [('GET', '/', b'', 200, b'Hello, Dodo!\n'),
                 ('HEAD', '/', b'', 200, b''), ('GET', '/missing', b'', 404, b''),
                 ('DELETE', '/', b'', 405, b''), ('POST', '/echo', b'abcde', 200, b'abcde'),
                 ('GET', '/fail', b'', 500, b''), ('POST', '/echo', b'x' * 33, 413, b''),
                 ('headers', '/', b'', 431, b''), ('malformed', '/', b'', 400, b''),
                 ('timeout', '/', b'', 408, b''), ('body_timeout', '/', b'', 408, b''),
                 ('continue', '/echo', b'abc', 200, b'abc'), ('GET', '/', b'', 200, b'Hello, Dodo!\n')]
        if cancellation:
            cases = [('cancel', '/', b'', 0, b'')]
        for method, path, body, status, expected in cases:
            raw = connect(port, process)
            with raw:
                stream = context.wrap_socket(raw, server_hostname='localhost') if context else raw
                with stream:
                    stream.settimeout(3)
                    if method == 'cancel':
                        stream.sendall(b'GET / HTTP/1.1\r\nHost:')
                        assert stream.recv(1024) == b''
                        break
                    if method == 'headers':
                        request = b'GET / HTTP/1.1\r\nHost: localhost\r\nX-Large: ' + b'a' * 600 + b'\r\n\r\n'
                    elif method == 'malformed':
                        request = b'POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n'
                    elif method == 'timeout':
                        request = b'GET / HTTP/1.1\r\nHost:'
                    elif method == 'body_timeout':
                        request = b'POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Length: 3\r\n\r\na'
                    elif method == 'continue':
                        stream.sendall(b'POST /echo HTTP/1.1\r\nHost: localhost\r\nExpect: 100-continue\r\nContent-Length: 3\r\n\r\n')
                        interim = b''
                        while not interim.endswith(b'\r\n\r\n'):
                            interim += stream.recv(1)
                        assert interim.startswith(b'HTTP/1.1 100 ')
                        request = b'abc'
                    elif method == 'POST' and len(body) == 5:
                        request = b'POST /echo HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\nX-Trailer: yes\r\n\r\n'
                    else:
                        request = f'{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {len(body)}\r\n\r\n'.encode() + body
                    # Fragment every ordinary head/body across independent sends.
                    for offset in range(0, len(request), 7):
                        try:
                            stream.sendall(request[offset:offset + 7])
                        except (BrokenPipeError, ConnectionResetError):
                            assert status >= 400
                            break
                    response = http.client.HTTPResponse(stream, method=method)
                    response.begin()
                    assert response.status == status, (method, response.status, status)
                    assert response.read() == expected, method
                    if method == 'HEAD':
                        assert response.getheader('Content-Length') == '13'
                    assert response.getheader('Connection') == 'close'
                    try:
                        assert stream.recv(1) == b''
                    except ConnectionResetError:
                        assert status >= 400, method
        stdout, stderr = process.communicate(timeout=10)
        assert process.returncode == 0, (stdout, stderr, process.returncode)
    finally:
        if process.poll() is None:
            process.kill()
        process.communicate(timeout=5)
    # TIME_WAIT may prevent immediate rebind (the native adapter does not set
    # SO_REUSEADDR). It must never leave a listening descriptor behind.
    with socket.socket() as probe:
        assert probe.connect_ex(('127.0.0.1', port)) == errno.ECONNREFUSED


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', type=Path, default=ROOT / 'target/debug/dodo')
    parser.add_argument('--skip-tls', action='store_true')
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    records = []
    with tempfile.TemporaryDirectory(prefix='dodo-hosted-http-') as directory:
        work = Path(directory)
        if not args.skip_tls:
            credentials(work)
        def build(fixture, level, substitutions=None, text=None):
            content = text if text is not None else (ROOT / f'tests/http_hosted/{fixture}.dodo').read_text()
            for key, value in (substitutions or {}).items():
                content = content.replace(f'@@{key}@@', str(value))
                if key == 'COUNT':
                    content = content.replace('const TEST_COUNT: usize = 0', f'const TEST_COUNT: usize = {value}')
                if key == 'STOP':
                    content = content.replace('const TEST_STOP: u64 = 30000', f'const TEST_STOP: u64 = {value}')
            source = work / f'{fixture}-{level}.dodo'
            source.write_text(content)
            executable = source.with_suffix('')
            run([args.compiler.resolve(), 'build', source, '-O', level, '-o', executable])
            return executable
        for level in (0, 3):
            run([build('driving', level)])
            records.append({'case': 'partial writes, fragmented parser, deadlines', 'optimization': level})
            peer = Peer()
            try:
                peer.execute(build('client', level, {'HTTP': peer.port}))
            finally:
                peer.close()
            records.append({'case': 'HTTP client, redirects, resolver scope, cleanup', 'optimization': level})
            port = available_port()
            executable = build('server', level, {'PORT': port, 'COUNT': 13, 'STOP': 30000})
            server_checks(executable, port)
            records.append({'case': 'serial server, limits, errors, HEAD, repeated connections', 'optimization': level})
            port = available_port()
            content = (ROOT / 'tests/http_hosted/server.dodo').read_text().replace('config.header_timeout_ms = 200', 'config.header_timeout_ms = 10000')
            executable = build('cancel', level, {'PORT': port, 'COUNT': 0, 'STOP': 500}, content)
            server_checks(executable, port, cancellation=True)
            records.append({'case': 'active cancellation, listener cleanup', 'optimization': level})
            if not args.skip_tls:
                roots = (work / 'ca.pem').read_text().replace('\n', '\\n')
                peer = Peer(work)
                try:
                    peer.execute(build('tls', level, {'TLS': peer.port, 'CA': roots}))
                finally:
                    peer.close()
                records.append({'case': 'HTTPS client, chain/hostname rejection, repeated cleanup', 'optimization': level})
                port = available_port()
                content = (ROOT / 'tests/http_hosted/server.dodo').read_text()
                content = content.replace('import "std/console"', 'import "std/console"\nimport "std/http/https"\nimport "std/tls/openssl"')
                cert = (work / 'cert.pem').read_text().replace('\n', '\\n')
                key = (work / 'key.pem').read_text().replace('\n', '\\n')
                content = content.replace('acceptor := hosted.PlainAcceptor {}', f'acceptor := https.Acceptor.new(openssl.Config.server(b"{cert}", b"{key}"))')
                executable = build('tls_server', level, {'PORT': port, 'COUNT': 13, 'STOP': 30000}, content)
                context = ssl.create_default_context(cafile=str(work / 'ca.pem'))
                context.set_alpn_protocols(['http/1.1'])
                server_checks(executable, port, context)
                records.append({'case': 'HTTPS hosted server / independent verified Python client', 'optimization': level})
                port = available_port()
                server_source = (ROOT / 'examples/https_server.dodo').read_text().replace(':8443', f':{port}').replace('.run_with(', '.max_connections(1).run_with(')
                client_source = (ROOT / 'examples/https_client.dodo').read_text().replace(':8443', f':{port}')
                server_exe = build('https_example_server', level, text=server_source)
                client_exe = build('https_example_client', level, text=client_source)
                process = subprocess.Popen([str(server_exe)], cwd=work, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    # Avoid a probe: this example admits one connection.
                    until = time.monotonic() + 5
                    while True:
                        result = subprocess.run([str(client_exe)], cwd=work, capture_output=True, timeout=10)
                        if result.returncode == 0:
                            assert result.stdout == b'Hello, Dodo!\n'
                            break
                        if process.poll() is not None or time.monotonic() >= until:
                            raise RuntimeError((result.returncode, result.stderr, process.communicate()))
                        time.sleep(.02)
                    stdout, stderr = process.communicate(timeout=5)
                    assert process.returncode == 0, (stdout, stderr)
                finally:
                    if process.poll() is None:
                        process.kill()
                    process.communicate(timeout=5)
                records.append({'case': 'complete HTTPS runtime-file examples', 'optimization': level})
            for example in ('http_client', 'web_server', 'https_client', 'https_server'):
                if args.skip_tls and example.startswith('https_'):
                    continue
                run([args.compiler.resolve(), 'build', ROOT / f'examples/{example}.dodo',
                     '--emit', 'obj', '--target', 'x86_64-pc-windows-msvc', '-O', level,
                     '-o', work / f'{example}-{level}.obj'])
            records.append({'case': 'Windows x64 object emission for hosted examples', 'optimization': level})
            print(f'PASS hosted HTTP{"/HTTPS" if not args.skip_tls else ""} -O{level}', flush=True)
    if args.report:
        args.report.write_text(json.dumps({'checks': len(records), 'records': records}, indent=2) + '\n')
    print(f'Passed {len(records)} hosted integration groups.')


if __name__ == '__main__':
    main()
