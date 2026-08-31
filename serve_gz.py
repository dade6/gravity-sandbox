#!/usr/bin/env python3
"""Server statico per gravity-sandbox con supporto gzip precompresso.

Serve i file da wasm-dist con Content-Encoding: gzip quando:
  - esiste <file>.gz precompresso, E
  - il client accetta gzip (Accept-Encoding), E
  - il file non è .gz stesso
Il WASM (110MB) comprime a ~15MB -> il download da remoto (iPhone/Mac via Tailscale)
non si interrompe più a metà.
Uso: python3 serve_gz.py [porta] [directory]  (log su stdout)
"""
import gzip
import os
import sys
import time
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8081
DIR = sys.argv[2] if len(sys.argv) > 2 else os.path.dirname(os.path.abspath(__file__))


class GzHandler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=DIR, **kwargs)

    def end_headers(self):
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()

    def send_head(self):
        # Se il client accetta gzip e il .gz esiste, serviamo il precompresso.
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

    def guess_type(self, path):
        if path.endswith(".wasm") or path.endswith(".wasm.gz"):
            return "application/wasm"
        return super().guess_type(path)

    def log_message(self, fmt, *args):
        sys.stdout.write("%s - - [%s] %s\n" % (self.address_string(),
                                               self.log_date_time_string(),
                                               fmt % args))
        sys.stdout.flush()


if __name__ == "__main__":
    os.chdir(DIR)
    server = ThreadingHTTPServer(("0.0.0.0", PORT), GzHandler)
    print(f"Serving {DIR} on 0.0.0.0:{PORT} (gzip precompresso attivo)", flush=True)
    server.serve_forever()
