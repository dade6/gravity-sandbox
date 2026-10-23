#!/usr/bin/env python3
"""Riceve i log di movimento della sandbox (`POST /log?s=<sessione>`) e li
appende su file, così i valori per-frame non vanno letti a occhio dal badge.

Il WASM non ha thread né filesystem: la sandbox accumula le righe in RAM
(`src/systems/motion_log.rs`), il polling JS le svuota e le spedisce qui; il
file vive sulla VM in `~/gravity-logs/motion-<sessione>.csv`.

Usato da `serve_gz.py` (HTTP 8081) e `serve-https.py` (HTTPS 9443).
"""
import os

LOG_DIR = os.path.expanduser(os.environ.get("GRAVITY_LOG_DIR", "~/gravity-logs"))

# Deve combaciare con l'ordine dei campi scritti da `motion_log.rs`.
HEADER = (
    "t_ms,dt_ms,zoom,cam_x,cam_y,followed,body_idx,body_x,body_y,"
    "phys_x,phys_y,gap,fract,ease,speed,star_idx,star_x,star_y\n"
)

MAX_BYTES = 64 * 1024 * 1024  # ~500k righe: oltre, il file viene ruotato


def _safe_session(session: str) -> str:
    keep = "".join(c for c in (session or "") if c.isalnum() or c in "-_")
    return keep[:64] or "default"


def append_log(session: str, body: bytes) -> str:
    """Appende `body` a motion-<sessione>.csv (creandolo con l'header)."""
    os.makedirs(LOG_DIR, exist_ok=True)
    path = os.path.join(LOG_DIR, f"motion-{_safe_session(session)}.csv")
    is_new = not os.path.exists(path)
    # Rotazione: un file troppo grande si sposta in .1 (una sola generazione).
    if not is_new and os.path.getsize(path) > MAX_BYTES:
        try:
            os.replace(path, path + ".1")
            is_new = True
        except OSError:
            pass
    if not body.endswith(b"\n"):
        body += b"\n"
    with open(path, "ab") as fh:
        if is_new:
            fh.write(HEADER.encode())
        fh.write(body)
    return path


def session_from_path(path: str) -> str:
    """Estrae `s=<...>` da '/log?s=abc'."""
    if "?" not in path:
        return "default"
    query = path.split("?", 1)[1]
    for part in query.split("&"):
        if part.startswith("s="):
            return part[2:]
    return "default"
