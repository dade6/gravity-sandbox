#!/usr/bin/env python3
"""Rimuove la sezione custom `name` da un .wasm (in place, atomico).

Perche': rustc per wasm32 emette la sezione dei nomi di funzione anche nei
build release. Nel wasm di gravity-sandbox sono ~55MB su 111MB totali —
meta' del download che il browser non usa a runtime (serve solo a stack
trace leggibili). Il wasm resta perfettamente valido: la sezione `name` e'
opzionale per la spec.

Uso: python3 scripts/strip_wasm_name.py wasm-dist/gravity_sandbox_bg.wasm
"""
import os
import sys
import tempfile


def read_uleb(d, i):
    result = 0
    shift = 0
    while True:
        b = d[i]
        i += 1
        result |= (b & 0x7f) << shift
        shift += 7
        if not b & 0x80:
            return result, i


def strip_sections(data, drop):
    """Ricompone il wasm saltando le custom section in `drop`."""
    if data[:4] != b"\0asm":
        raise SystemExit("non e' un file wasm")
    out = bytearray(data[:8])
    i = 8
    removed = 0
    while i < len(data):
        start = i
        section_id = data[i]
        i += 1
        size, i = read_uleb(data, i)
        name = None
        if section_id == 0:
            name_len, j = read_uleb(data, i)
            name = data[j:j + name_len].decode("utf-8", "replace")
        i += size
        if i > len(data):
            raise SystemExit("sezione troncata: file wasm corrotto")
        if name in drop:
            removed += i - start
            continue
        out += data[start:i]
    return bytes(out), removed


def main():
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    path = sys.argv[1]
    drops = set(sys.argv[2:]) or {"name"}
    data = open(path, "rb").read()
    # Conserva i permessi originali ma garantisce la lettura: mkstemp crea 0600.
    mode = (os.stat(path).st_mode & 0o777) | 0o444
    stripped, removed = strip_sections(data, drops)
    if removed == 0:
        print(f"{path}: sezione {sorted(drops)} assente, nessuna modifica")
        return
    # scrittura atomica: il server puo' servire il file mentre lo sostituiamo
    fd, tmp = tempfile.mkstemp(dir=os.path.dirname(os.path.abspath(path)), suffix=".tmp")
    try:
        with os.fdopen(fd, "wb") as f:
            f.write(stripped)
        os.chmod(tmp, mode)
        os.replace(tmp, path)
    except BaseException:
        os.unlink(tmp)
        raise
    print(
        f"{path}: {len(data):,} -> {len(stripped):,} bytes "
        f"(-{removed:,} = -{removed / 1e6:.1f}MB)"
    )


if __name__ == "__main__":
    main()
