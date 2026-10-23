#!/usr/bin/env python3
"""Servizio di asset precompressi (brotli/gzip) + politiche di cache.

Modulo condiviso da serve_gz.py (8081) e serve-https.py (9443).

Perche' brotli: sul wasm di gravity-sandbox (Bevy) brotli q11 fa ~6.6MB
contro i ~12.4MB di gzip -9, cioe' quasi la meta' del tempo di download
sulla connessione lenta dell'iPhone. Il .br si genera in build_wasm.sh
(scripts/brotli_compress.js); se manca si ricade su .gz, e se manca
anche quello il file va in chiaro.

Cache: le URL "pesanti" portano un cache-buster ?v=<versione> (index.html).
Su quelle rispondiamo `immutable`: il browser (Safari iOS incluso) tiene
il wasm su disco e le ricariche successive non riscaricano nulla. Ogni
build bumpa la versione, quindi un wasm nuovo ha sempre una URL nuova.
Senza ?v= resta `no-cache` (revalida con If-Modified-Since -> 304).
"""
import os
from email.utils import parsedate_to_datetime
from http.server import SimpleHTTPRequestHandler

# Ordine di preferenza: brotli prima (piu' piccolo), poi gzip.
# La prima che il client accetta E che esiste su disco vince.
VARIANTS = ((".br", "br"), (".gz", "gzip"))

# Asset versionabili con ?v=: cache lunga e immutabile.
IMMUTABLE_EXTS = (".wasm", ".js", ".size", ".d.ts")


def _accepts(header, token):
    """True se Accept-Encoding contiene davvero il token (non un prefisso)."""
    for part in (header or "").lower().split(","):
        enc = part.split(";")[0].strip()
        if enc == token:
            return True
    return False


def _cache_control(path_with_query):
    """Politica di cache per la richiesta corrente."""
    path_only, _, query = path_with_query.partition("?")
    if "v=" in query and path_only.lower().endswith(IMMUTABLE_EXTS):
        # Contenuto immutabile: la versione e' nella URL.
        return "public, max-age=31536000, immutable"
    return "no-cache"


class PrecompressedMixin(SimpleHTTPRequestHandler):
    """Handler statico con varianti brotli/gzip precompresse e cache corretta.

    Sottoclasse direttamente SimpleHTTPRequestHandler: chi la usa eredita
    solo questa (vedi serve_gz.GzHandler / serve-https.Handler).
    """

    # keep-alive: senza, ogni asset paga un handshake TCP+TLS in piu'
    # (su Tailscale/DERP ~100-200ms l'uno) e il wasm non riusa la connessione.
    protocol_version = "HTTP/1.1"

    def _cache_control_value(self):
        return _cache_control(self.path)

    def end_headers(self):
        self.send_header("Cache-Control", self._cache_control_value())
        super().end_headers()

    def _pick_variant(self):
        """Ritorna (percorso_del_variante, content_encoding) o None."""
        if self.path.endswith(".br") or self.path.endswith(".gz"):
            return None  # gia' un file compresso: niente doppia codifica
        ae = self.headers.get("Accept-Encoding") or ""
        base = self.translate_path(self.path.split("?", 1)[0])
        try:
            source_mtime = os.path.getmtime(base)
        except OSError:
            source_mtime = None
        for ext, enc in VARIANTS:
            candidate = base + ext
            if not (_accepts(ae, enc) and os.path.isfile(candidate)):
                continue
            # GUARDIA ANTI-STANTIO: se il sorgente e' stato aggiornato dopo la
            # variante, quella variante contiene la versione PRECEDENTE del
            # file. Servirla (con Cache-Control: no-cache + 304) incolla il
            # browser alla build vecchia: e' successo con index.html.br/.gz
            # rimasti alla 0.14.161 mentre il wasm era alla 0.14.163 ("in
            # console vedo ancora la 161"). Meglio il file in chiaro: piu'
            # grosso ma CORRETTO.
            if source_mtime is not None and os.path.getmtime(candidate) < source_mtime:
                print(
                    f"[stale-variant] {candidate} precedente a {base}: servito in chiaro",
                    flush=True,
                )
                continue
            return candidate, enc
        return None

    def send_head(self):
        picked = self._pick_variant()
        if picked is None:
            return super().send_head()

        variant_path, encoding = picked
        # Il contenuto "vero" e' il file NON compresso: la sua mtime decide la
        # revalidation (ricomprimere non cambia il contenuto da mostrare).
        try:
            content_mtime = os.path.getmtime(self.translate_path(self.path.split("?", 1)[0]))
        except OSError:
            content_mtime = None
        try:
            f = open(variant_path, "rb")
        except OSError:
            return super().send_head()

        try:
            st = os.fstat(f.fileno())
            last_modified = content_mtime if content_mtime is not None else st.st_mtime
            # Revalidation: se il client ha gia' questa versione, 304 senza
            # corpo (vale per gli asset non versionati).
            ims = self.headers.get("If-Modified-Since")
            not_modified = False
            if ims:
                try:
                    ims_ts = parsedate_to_datetime(ims).timestamp()
                    not_modified = int(last_modified) <= int(ims_ts)
                except (TypeError, ValueError, OverflowError):
                    not_modified = False
            if not_modified:
                f.close()
                self.send_response(304)
                self.send_header("Vary", "Accept-Encoding")
                self.end_headers()
                return None

            self.send_response(200)
            # Il tipo MIME e' quello del file NON compresso.
            self.send_header("Content-Type", self.guess_type(self.path.split("?", 1)[0]))
            self.send_header("Content-Encoding", encoding)
            self.send_header("Content-Length", str(st.st_size))
            self.send_header("Last-Modified", self.date_time_string(last_modified))
            # Vary: senza, una cache intermedia potrebbe servire il .br a un
            # client che non lo accetta.
            self.send_header("Vary", "Accept-Encoding")
            self.end_headers()
        except OSError:
            f.close()
            return super().send_head()
        return f
