"""Independent Python ssl peer for Dodo HTTP over verified TLS, on Linux or Wine."""
import pathlib
import socket
import ssl
import subprocess
import sys
import threading


class LoopbackPeer:
    def __init__(self, work):
        self.work = pathlib.Path(work)
        self.context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        self.context.load_cert_chain(self.work / "cert.pem", self.work / "key.pem")
        self.context.set_alpn_protocols(["http/1.1"])
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(1)
        self.port = self.listener.getsockname()[1]

    def source(self, fixture):
        source = pathlib.Path(fixture).read_text().replace(
            "const PORT: u16 = 0", f"const PORT: u16 = {self.port}")
        source = source.replace("@@PORT@@", str(self.port))
        return source.replace("@@CA@@", (self.work / "ca.pem").read_text().replace("\n", "\\n"))

    def run(self, execute):
        errors = []

        def serve():
            try:
                self.listener.settimeout(20)
                raw, _ = self.listener.accept()
                raw.settimeout(15)
                with self.context.wrap_socket(raw, server_side=True) as peer:
                    assert peer.selected_alpn_protocol() == "http/1.1"
                    request = bytearray()
                    while b"\r\n\r\n" not in request:
                        data = peer.recv(3)
                        assert data
                        request.extend(data)
                        assert len(request) < 1024
                    assert request.startswith(b"GET / HTTP/1.1\r\n")
                    assert f"Host: localhost:{self.port}\r\n".encode() in request
                    assert b"Connection: close\r\n" in request
                    for byte in b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello":
                        peer.sendall(bytes([byte]))
                    # Both sides complete authenticated shutdown after HTTP framing.
                    peer.unwrap().close()
            except BaseException as error:
                errors.append(error)
            finally:
                self.close()

        worker = threading.Thread(target=serve, daemon=True)
        worker.start()
        try:
            execute()
        finally:
            worker.join(20)
        assert not worker.is_alive(), "TLS peer did not finish"
        if errors:
            raise errors[0]

    def close(self):
        self.listener.close()


def main():
    compiler, directory, fixture, level = sys.argv[1:]
    work = pathlib.Path(directory)
    peer = LoopbackPeer(work)
    try:
        path = work / f"loopback-{level}.dodo"
        path.write_text(peer.source(fixture))
        exe = work / f"loopback-{level}"
        subprocess.run([compiler, "build", str(path), "-O", level, "-o", str(exe)],
                       check=True, timeout=600)
        peer.run(lambda: subprocess.run([str(exe)], check=True, timeout=25))
    finally:
        peer.close()


if __name__ == "__main__":
    main()
