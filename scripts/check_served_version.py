#!/usr/bin/env python3
"""Verifica che gli asset SERVITI siano davvero quelli su disco.

Nasce da un deploy sbagliato (v0.14.163): il wasm era nuovo ma i file
precompressi di `index.html` (`.br`/`.gz`) erano rimasti alla 0.14.161, e il
browser — che chiede brotli — riceveva la pagina VECCHIA. `curl` senza
`Accept-Encoding` non se ne accorge: serve chiedere esplicitamente le stesse
codifiche del browser.

Controlla, su HTTP (8081) e HTTPS (9443):
  1. `index.html` servito con brotli e con gzip contiene i cache-buster della
     versione in `src/version.rs`;
  2. il wasm servito (brotli e gzip) ha lo stesso sha256 del file su disco;
  3. `gravity_sandbox_bg.wasm.size` coincide con la dimensione su disco.

Uso:
    python3 scripts/check_served_version.py [--version v0.14.163]

Exit code 0 = tutto allineato, 1 = qualcosa serve una versione vecchia
(il caso classico: precompressione dimenticata dopo il build).
"""
import argparse
import hashlib
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WASM = ROOT / "wasm-dist" / "gravity_sandbox_bg.wasm"

# (nome, base URL, flag extra per curl)
ORIGINS = (
    ("http", "http://localhost:8081", []),
    ("https", "https://localhost:9443", ["-k"]),
)

ENCODINGS = ("br", "gzip")


def curl(url: str, encoding: str, extra: list[str]) -> bytes:
    """Scarica `url` chiedendo `encoding` e DECOMPRIMENDO il corpo."""
    cmd = [
        "curl",
        "-s",
        "--compressed",  # fa decomprimere da curl (br e gzip)
        "-H",
        f"Accept-Encoding: {encoding}",
        *extra,
        url,
    ]
    out = subprocess.run(cmd, capture_output=True, timeout=120)
    if out.returncode != 0:
        raise RuntimeError(f"curl fallito su {url} ({encoding}): {out.returncode}")
    return out.stdout


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", help="versione attesa (default: src/version.rs)")
    args = parser.parse_args()

    if shutil.which("curl") is None:
        print("ERRORE: serve curl", file=sys.stderr)
        return 1

    if args.version:
        version = args.version
    else:
        text = (ROOT / "src" / "version.rs").read_text(encoding="utf-8")
        found = re.search(r'"(v[0-9.]+)"', text)
        if found is None:
            print("ERRORE: versione non trovata in src/version.rs", file=sys.stderr)
            return 1
        version = found.group(1)
    buster = f"?v={version.lstrip('v')}"
    print(f"versione attesa: {version} (cache-buster {buster})")

    if not WASM.is_file():
        print(f"ERRORE: manca {WASM}", file=sys.stderr)
        return 1
    disk_sha = hashlib.sha256(WASM.read_bytes()).hexdigest()
    disk_size = WASM.stat().st_size

    failures = 0

    def check(label: str, ok: bool, detail: str = "") -> None:
        nonlocal failures
        print(f"  {'OK  ' if ok else 'FAIL'} {label}{' — ' + detail if detail and not ok else ''}")
        if not ok:
            failures += 1

    for name, base, extra in ORIGINS:
        print(f"[{name}] {base}")
        for enc in ENCODINGS:
            try:
                html = curl(f"{base}/index.html", enc, extra).decode("utf-8", "replace")
            except Exception as exc:  # noqa: BLE001 - diagnostica, non logica
                check(f"index.html ({enc})", False, str(exc))
                continue
            refs = set(re.findall(r"gravity_sandbox(?:_bg\.wasm|\.js)\?v=[0-9.]+", html))
            stale = sorted(r for r in refs if buster not in r)
            check(
                f"index.html ({enc}) → {buster}",
                bool(refs) and not stale,
                f"trovati {sorted(refs) or 'nessun riferimento'}",
            )

            try:
                served = curl(f"{base}/gravity_sandbox_bg.wasm", enc, extra)
            except Exception as exc:  # noqa: BLE001
                check(f"wasm ({enc})", False, str(exc))
                continue
            sha = hashlib.sha256(served).hexdigest()
            check(
                f"wasm ({enc}) sha256 = disco",
                sha == disk_sha,
                f"servito {sha[:16]}… disco {disk_sha[:16]}…",
            )

        try:
            size_txt = curl(f"{base}/gravity_sandbox_bg.wasm.size", "gzip", extra)
            served_size = int(size_txt.strip() or b"0")
        except Exception as exc:  # noqa: BLE001
            check("wasm.size", False, str(exc))
            served_size = 0
        check(
            f"wasm.size = {disk_size}",
            served_size == disk_size,
            f"servito {served_size}",
        )

    if failures:
        print(f"\n{failures} controllo/i falliti: la pagina servita NON è la {version}.")
        print("Causa tipica: file precompressi (.br/.gz) non rigenerati dopo il build.")
        print("Rimedio: rigenerare TUTTI i .gz/.br (vedi build_wasm.sh) o usare build_wasm.sh.")
        return 1

    print(f"\nTutto allineato: HTTP e HTTPS servono la {version} (brotli e gzip).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
