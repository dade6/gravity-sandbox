#!/usr/bin/env python3
"""HTTP server for gravity-sandbox: WASM MIME + POST /save-preset + gzip.

GET: serve static files (like SimpleHTTPRequestHandler) from DIR.
For .wasm/.js a precompressed <file>.gz is served with Content-Encoding:
gzip when the client accepts it (cuts 80-110MB WASM transfers ~4x on slow
mobile/Tailscale links). Progress-friendly: Content-Length always set.

POST /save-preset: writes the request body to assets/preset.json (the
source file, resolved relative to DIR's parent). Used by the in-app
"Save" button (Bevy UI) so Davide can overwrite the preset directly
from the sandbox, without editing files on the server.
"""
import http.server
import json
import os
import shutil
import sys

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8080
DIR = sys.argv[2] if len(sys.argv) > 2 else '.'
DIR = os.path.abspath(DIR)  # assoluta: _resolve confronta con abspath
# Il preset sorgente vive in assets/preset.json, ovvero <DIR>/../assets/preset.json
# quando DIR è wasm-dist. Normpath per gestire '..' correttamente.
PRESET_PATH = os.path.normpath(os.path.join(DIR, '..', 'assets', 'preset.json'))
PRESET_DEFAULT = os.path.normpath(os.path.join(DIR, '..', 'assets', 'preset.default.json'))


def ensure_preset_exists():
    """Se assets/preset.json non esiste (fresh clone / primo avvio), lo crea
    dal default distribuito (assets/preset.default.json)."""
    if not os.path.isfile(PRESET_PATH) and os.path.isfile(PRESET_DEFAULT):
        try:
            import shutil
            shutil.copyfile(PRESET_DEFAULT, PRESET_PATH)
            print(f'preset.json creato da preset.default.json', flush=True)
        except OSError as e:
            print(f'WARN: impossibile creare preset.json: {e}', flush=True)


class SandboxHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, fmt, *args):
        # come SimpleHTTPRequestHandler (più lo stato)
        sys.stderr.write("%s - - [%s] %s\n" % (
            self.address_string(), self.log_date_time_string(), fmt % args))

    def guess_type(self, path):
        if path.endswith('.wasm'):
            return 'application/wasm'
        if path.endswith('.js'):
            return 'application/javascript'
        import mimetypes
        return mimetypes.guess_type(path)[0] or 'application/octet-stream'

    def _resolve(self, rel):
        # risolve la richiesta su un file, con fallback index.html
        path = os.path.normpath(os.path.join(DIR, rel.lstrip('/')))
        if not path.startswith(os.path.abspath(DIR)):
            return None  # path traversal -> 404
        if os.path.isdir(path):
            path = os.path.join(path, 'index.html')
        return path

    def do_GET(self):
        self._serve(head_only=False)

    def do_HEAD(self):
        self._serve(head_only=True)

    def do_OPTIONS(self):
        # Preflight CORS: Safari (iOS/Mac) manda un OPTIONS prima del fetch
        # del WASM quando considera la richiesta cross-origin (rotte
        # tailscale/serve su porte diverse). Senza questo handler il server
        # risponde 501 senza header CORS -> "Fetch API cannot load ... due
        # to access control checks" anche se la GET ha Access-Control-Allow-Origin.
        self.send_response(204)
        self.send_header('Access-Control-Allow-Origin', '*')
        self.send_header('Access-Control-Allow-Methods', 'GET, HEAD, POST, OPTIONS')
        self.send_header('Access-Control-Allow-Headers', '*')
        self.send_header('Access-Control-Max-Age', '86400')
        self.end_headers()

    def _serve(self, head_only=False):
        path = self._resolve(self.path.split('?')[0])
        if path is None or not os.path.isfile(path):
            self.send_error(404, 'Not Found')
            return

        gz = path + '.gz'
        accept_gz = 'gzip' in self.headers.get('Accept-Encoding', '')
        # Serve .gz solo per asset grossi tipici del WASM e solo se esiste.
        want_gz = path.endswith('.wasm') or path.endswith('.js')
        serve_gz = want_gz and accept_gz and os.path.isfile(gz)

        real = gz if serve_gz else path
        size = os.path.getsize(real)

        self.send_response(200)
        self.send_header('Content-Type', self.guess_type(path))
        self.send_header('Content-Length', str(size))
        self.send_header('Cache-Control', 'no-store')
        # Safari può tassonomizzare il fetch del WASM come cross-origin
        # (rotte tailscale/serve multi-porta): header CORS espliciti.
        self.send_header('Access-Control-Allow-Origin', '*')
        if serve_gz:
            self.send_header('Content-Encoding', 'gzip')
            self.send_header('Vary', 'Accept-Encoding')
        self.end_headers()

        if head_only:
            return
        with open(real, 'rb') as f:
            shutil.copyfileobj(f, self.wfile, length=1024 * 1024)

    def do_POST(self):
        if self.path != '/save-preset':
            self.send_error(404, 'Not Found')
            return
        length = int(self.headers.get('Content-Length', 0))
        if length <= 0:
            self.send_error(400, 'Empty body')
            return
        body = self.rfile.read(length)
        # Valida che sia JSON valido prima di scrivere (niente file corrotti)
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
        # Risposta JSON di conferma (l'utente la vede nel debug/console)
        self.send_response(200)
        self.send_header('Content-type', 'application/json')
        self.send_header('Cache-Control', 'no-store')
        self.end_headers()
        self.wfile.write(json.dumps({'ok': True, 'path': PRESET_PATH}).encode())


if __name__ == '__main__':
    ensure_preset_exists()
    print(f'Serving {DIR} on 0.0.0.0:{PORT} | save-preset -> {PRESET_PATH} | gzip on', flush=True)
    http.server.ThreadingHTTPServer(('0.0.0.0', PORT), SandboxHandler).serve_forever()
