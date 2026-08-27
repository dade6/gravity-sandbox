#!/usr/bin/env bash
# Build WASM + copia gli asset modificabili (shaders, preset.json, index.html)
# in wasm-dist/. Gli asset serviti via HTTP si possono modificare sul server e
# ricaricare la pagina SENZA ricompilare il WASM.
#
# NB: il glue JS cerca il wasm col path INTERNO derivato dal nome del crate
# (gravity_sandbox_bg.wasm) — una rinomina --out-name versionata rende glue e
# file incoerenti (404). La freschezza della build è garantita da:
#   - Cache-Control: no-store sul server (serve_wasm.py)
#   - cache-buster ?v=<versione> sull'import del glue JS in index.html
#   - Access-Control-Allow-Origin: * (Safari / rotte Tailscale)
#
# Uso: ./build_wasm.sh
set -euo pipefail
cd "$(dirname "$0")"

# Pulizia file versionati residui (dalla breve era dei nomi versionati)
rm -f wasm-dist/gravity_sandbox_[0-9]*.js \
      wasm-dist/gravity_sandbox_[0-9]*_bg.wasm \
      wasm-dist/gravity_sandbox_[0-9]*.d.ts \
      wasm-dist/gravity_sandbox_[0-9]*_bg.wasm.d.ts \
      wasm-dist/gravity_sandbox.js.gz wasm-dist/gravity_sandbox_bg.wasm.gz

# Build WASM: --no-opt evita il timeout di wasm-opt su binary ~110MB.
# Target dir condiviso configurato in .cargo/config.toml.
RUSTFLAGS='--cfg getrandom_backend="wasm_js"' \
    wasm-pack build --target web --no-opt --out-dir wasm-dist --out-name gravity_sandbox

# Gzip precompresso del wasm: da iPhone (Tailscale/DERP) il fetch di 112MB si
# tronca a metà e Safari lo segnala come errore CORS ("due to access control
# checks"); il server serve il .gz (Content-Encoding: gzip) a chi lo accetta.
# 16.8MB invece di 112MB -> il fetch passa anche su connessioni lente.
gzip -kf wasm-dist/gravity_sandbox_bg.wasm

# Sidecar progresso caricamento: taglia DECOMPRESSA del wasm (quello che il
# reader del browser emette anche quando il server manda il .gz). index.html
# lo legge per mostrare la % REALE di download sotto gzip (Content-Length è la
# taglia compressa, non confrontabile coi byte decompressi letti dal reader).
stat -c %s wasm-dist/gravity_sandbox_bg.wasm > wasm-dist/gravity_sandbox_bg.wasm.size

# Asset serviti via HTTP (modificabili senza ricompilare il WASM)
mkdir -p wasm-dist/assets
cp -r assets/shaders wasm-dist/assets/
cp -r assets/textures wasm-dist/assets/
# preset.json: usa un symlink invece di una copia, così il server serve SEMPRE
# il file sorgente assets/preset.json (modifiche visibili senza rebuild).
ln -sf ../../assets/preset.json wasm-dist/assets/preset.json
# index.html contiene il glue JS (fetch preset, badge versione): copiarlo
# esplicitamente per non dipendere dal comportamento di wasm-pack
cp index.html wasm-dist/index.html

echo "Build OK: wasm-dist aggiornato (gravity_sandbox + assets + index.html)"