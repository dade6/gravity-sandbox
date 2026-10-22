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
from http.server import ThreadingHTTPServer

from precompressed import PrecompressedMixin
from logsink import append_log, session_from_path

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


class GzHandler(PrecompressedMixin):
    """Handler con brotli/gzip precompresso (logica in precompressed.py)."""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=DIR, **kwargs)

    def guess_type(self, path):
        if path.endswith(".wasm") or path.endswith(".wasm.gz") or path.endswith(".wasm.br"):
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
        # Log di movimento per-frame: POST /log?s=<sessione> -> CSV su file.
        # Deve stare PRIMA del ramo /save-preset (che è l'unico altro POST).
        if self.path.split("?", 1)[0] == '/log':
            length = int(self.headers.get('Content-Length', 0))
            if length <= 0:
                self.send_error(400, 'Empty body')
                return
            body = self.rfile.read(length)
            try:
                path = append_log(session_from_path(self.path), body)
            except OSError as e:
                self.send_error(500, f'Write failed: {e}')
                return
            payload = json.dumps({'ok': True, 'path': path, 'bytes': len(body)}).encode()
            self.send_response(200)
            self.send_header('Content-type', 'application/json')
            self.send_header('Cache-Control', 'no-store')
            self.send_header('Access-Control-Allow-Origin', '*')
            self.send_header('Content-Length', str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
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
        payload = json.dumps({'ok': True, 'path': PRESET_PATH}).encode()
        self.send_response(200)
        self.send_header('Content-type', 'application/json')
        self.send_header('Cache-Control', 'no-store')
        self.send_header('Access-Control-Allow-Origin', '*')
        # HTTP/1.1 + keep-alive: senza Content-Length il client resterebbe
        # in attesa di altro corpo fino al timeout.
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


if __name__ == "__main__":
    ensure_preset_exists()
    os.chdir(DIR)
    server = ThreadingHTTPServer(("0.0.0.0", PORT), GzHandler)
    print(f"Serving {DIR} on 0.0.0.0:{PORT} | save-preset -> {PRESET_PATH} | gzip precompresso attivo", flush=True)
    server.serve_forever()
