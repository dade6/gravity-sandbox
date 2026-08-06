#!/usr/bin/env python3
"""HTTP server for gravity-sandbox: WASM MIME + POST /save-preset.

GET: serve static files (like SimpleHTTPRequestHandler) from DIR.
POST /save-preset: writes the request body to assets/preset.json (the
source file, resolved relative to DIR's parent). Used by the in-app
"Save" button (Bevy UI) so Davide can overwrite the preset directly
from the sandbox, without editing files on the server.
"""
import http.server
import json
import os
import sys

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8080
DIR = sys.argv[2] if len(sys.argv) > 2 else '.'
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


class SandboxHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=DIR, **kwargs)

    def guess_type(self, path):
        if path.endswith('.wasm'):
            return 'application/wasm'
        if path.endswith('.js'):
            return 'application/javascript'
        return super().guess_type(path)

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
        self.end_headers()
        self.wfile.write(json.dumps({'ok': True, 'path': PRESET_PATH}).encode())


if __name__ == '__main__':
    ensure_preset_exists()
    print(f'Serving {DIR} on 0.0.0.0:{PORT} | save-preset -> {PRESET_PATH}', flush=True)
    http.server.HTTPServer(('0.0.0.0', PORT), SandboxHandler).serve_forever()
