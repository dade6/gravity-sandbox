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
import json
import os
import shutil
import sys
import time
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8081
DIR = sys.argv[2] if len(sys.argv) > 2 else os.path.dirname(os.path.abspath(__file__))

# Il preset vive in <DIR>/../assets/preset.json
PRESET_PATH = os.path.normpath(os.path.join(DIR, '..', 'assets', 'preset.json'))
PRESET_DEFAULT = os.path.normpath(os.path.join(DIR, '..', 'assets', 'preset.default.json'))


def ensure_preset_exists():
    if not os.path.isfile(PRESET_PATH) and os.path.isfile(PRESET_DEFAULT):
        try:
            shutil.copyfile(PRESET_DEFAULT, PRESET_PATH)
            print(f'preset.json creato da preset.default.json', flush=True)
        except OSError as e:
            print(f'WARN: impossibile creare preset.json: {e}', flush=True)


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

    def do_OPTIONS(self):
        # Preflight CORS
        self.send_response(204)
        self.send_header('Access-Control-Allow-Origin', '*')
        self.send_header('Access-Control-Allow-Methods', 'GET, HEAD, POST, OPTIONS')
        self.send_header('Access-Control-Allow-Headers', '*')
        self.send_header('Access-Control-Max-Age', '86400')
        self.end_headers()

    def do_POST(self):
        if self.path != '/save-preset':
            self.send_error(404, 'Not Found')
            return
        length = int(self.headers.get('Content-Length', 0))
        if length <= 0:
            self.send_error(400, 'Empty body')
            return
        body = self.rfile.read(length)
        try:
            data = json.loads(body)
            if 'bodies' not in data:
                raise ValueError("missing 'bodies' key")
        except (ValueError, json.JSONDecodeError) as e:
            self.send_error(400, f'Invalid JSON: {e}')
            return
        try:
            with open(PRESET_PATH, 'wb') as f:
                f.write(body)
        except OSError as e:
            self.send_error(500, f'Write failed: {e}')
            return
        self.send_response(200)
        self.send_header('Content-type', 'application/json')
        self.send_header('Cache-Control', 'no-store')
        self.send_header('Access-Control-Allow-Origin', '*')
        self.end_headers()
        self.wfile.write(json.dumps({'ok': True, 'path': PRESET_PATH}).encode())


if __name__ == "__main__":
    ensure_preset_exists()
    os.chdir(DIR)
    server = ThreadingHTTPServer(("0.0.0.0", PORT), GzHandler)
    print(f"Serving {DIR} on 0.0.0.0:{PORT} | save-preset -> {PRESET_PATH} | gzip precompresso attivo", flush=True)
    server.serve_forever()
