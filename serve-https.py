#!/usr/bin/env python3
"""HTTPS static server for gravity-sandbox wasm-dist (WebGPU secure context).

Serve i file da wasm-dist con Content-Encoding: gzip quando esiste il
<file>.gz precompresso (il WASM da ~115MB scende a ~17MB).
Uso: python3 serve-https.py [porta] (default 9443)
"""
import json
import os
import shutil
import ssl
import sys
from http.server import ThreadingHTTPServer

from precompressed import PrecompressedMixin

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 9443
BASE = "/home/ubuntu/workspace/gravity-sandbox/wasm-dist"
CERT = "/home/ubuntu/workspace/gravity-sandbox/.tls/cert.crt"
KEY = "/home/ubuntu/workspace/gravity-sandbox/.tls/cert.key"

# Il preset vive in <BASE>/../assets/preset.json (stesso file del server 8081)
PRESET_PATH = os.path.normpath(os.path.join(BASE, "..", "assets", "preset.json"))
PRESET_DEFAULT = os.path.normpath(os.path.join(BASE, "..", "assets", "preset.default.json"))


def ensure_preset_exists():
    if not os.path.isfile(PRESET_PATH) and os.path.isfile(PRESET_DEFAULT):
        try:
            shutil.copyfile(PRESET_DEFAULT, PRESET_PATH)
            print("preset.json creato da preset.default.json", flush=True)
        except OSError as e:
            print(f"WARN: impossibile creare preset.json: {e}", flush=True)


class Handler(PrecompressedMixin):
    """HTTPS handler: brotli/gzip precompresso + cache (precompressed.py)."""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=BASE, **kwargs)

    def log_message(self, format, *args):
        pass  # quiet

    def do_OPTIONS(self):
        # Preflight CORS (stesso pattern di serve_gz.py)
        self.send_response(204)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, HEAD, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "*")
        self.send_header("Access-Control-Max-Age", "86400")
        self.end_headers()

    def do_POST(self):
        # Bottone "Salva" della sandbox: POST /save-preset -> assets/preset.json
        # (il SimpleHTTPRequestHandler base risponde 501 senza questo handler)
        print(f"POST {self.path}", flush=True)
        if self.path != "/save-preset":
            self.send_error(404, "Not Found")
            return
        length = int(self.headers.get("Content-Length", 0))
        if length <= 0:
            self.send_error(400, "Empty body")
            return
        body = self.rfile.read(length)
        try:
            data = json.loads(body)
            if "bodies" not in data:
                raise ValueError("missing 'bodies' key")
        except (ValueError, json.JSONDecodeError) as e:
            self.send_error(400, f"Invalid JSON: {e}")
            return
        try:
            with open(PRESET_PATH, "wb") as f:
                f.write(body)
        except OSError as e:
            self.send_error(500, f"Write failed: {e}")
            return
        print(f"preset salvato: {PRESET_PATH} ({len(body)} bytes)", flush=True)
        payload = json.dumps({"ok": True, "path": PRESET_PATH}).encode()
        self.send_response(200)
        self.send_header("Content-type", "application/json")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Access-Control-Allow-Origin", "*")
        # HTTP/1.1 + keep-alive: senza Content-Length il client aspetta altro corpo.
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


ensure_preset_exists()
httpd = ThreadingHTTPServer(("0.0.0.0", PORT), Handler)
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain(CERT, KEY)
httpd.socket = ctx.wrap_socket(httpd.socket, server_side=True)
print(f"HTTPS serving {BASE} on :{PORT}", flush=True)
httpd.serve_forever()
