"""Independent decoder: CPython's RFC 9112 implementation consumes Dodo output."""
import http.client
import io
import subprocess
import sys


class MemorySocket:
    def __init__(self, payload):
        self.payload = payload

    def makefile(self, mode):
        assert mode == "rb"
        return io.BytesIO(self.payload)


payload = subprocess.check_output([sys.argv[1]])
response = http.client.HTTPResponse(MemorySocket(payload))
response.begin()
assert response.status == 200
assert response.getheader("Content-Type") == "text/plain"
assert response.chunked
assert response.read(3) == b"hel"
assert response.read(2) == b"lo"
assert response.read() == b" from Dodo"
assert response.isclosed()
print("CPython HTTPResponse decoded Dodo headers, chunks, streaming body, and trailers")
