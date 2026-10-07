#!/usr/bin/env python3
"""HTTPS static server for gravity-sandbox wasm-dist (WebGPU secure context).

Serve i file da wasm-dist con Content-Encoding: gzip quando esiste il
<file>.gz precompresso (il WASM da ~115MB scende a ~17MB).
Uso: python3 serve-https.py [porta] (default 9443)
"""
import os
import ssl
import sys
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 9443
BASE = "/home/ubuntu/workspace/gravity-sandbox/wasm-dist"
CERT = "/home/ubuntu/workspace/gravity-sandbox/.tls/cert.crt"
KEY = "/home/ubuntu/workspace/gravity-sandbox/.tls/cert.key"


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=BASE, **kwargs)

    def end_headers(self):
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()

    def send_head(self):
        accepts_gzip = "gzip" in (self.headers.get("Accept-Encoding") or "")
        gz_path = self.translate_path(self.path) + ".gz"
        if accepts_gzip and not self.path.endswith(".gz") and os.path.isfile(gz_path):
            try:
                f = open(gz_path, "rb")
            except OSError:
                return super().send_head()
            ctype = self.guess_type(gz_path)
            self.send_response(200)
            self.send_header("Content-type", ctype)
            self.send_header("Content-Encoding", "gzip")
            self.send_header("Content-Length", str(os.fstat(f.fileno()).st_size))
            self.send_header("Last-Modified", self.date_time_string(os.fstat(f.fileno()).st_mtime))
            self.end_headers()
            return f
        return super().send_head()

    def log_message(self, format, *args):
        pass  # quiet


httpd = ThreadingHTTPServer(("0.0.0.0", PORT), Handler)
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain(CERT, KEY)
httpd.socket = ctx.wrap_socket(httpd.socket, server_side=True)
print(f"HTTPS serving {BASE} on :{PORT}", flush=True)
httpd.serve_forever()
