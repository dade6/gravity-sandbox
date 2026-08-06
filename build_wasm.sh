#!/usr/bin/env bash
# Build WASM + copia gli asset modificabili (shaders, preset.json, index.html)
# in wasm-dist/. Gli asset serviti via HTTP si possono modificare sul server e
# ricaricare la pagina SENZA ricompilare il WASM.
#
# Uso: ./build_wasm.sh
set -euo pipefail
cd "$(dirname "$0")"

# Build WASM: --no-opt evita il timeout di wasm-opt su binary ~110MB.
# Target dir condiviso configurato in .cargo/config.toml.
RUSTFLAGS='--cfg getrandom_backend="wasm_js"' \
    wasm-pack build --target web --no-opt --out-dir wasm-dist --out-name gravity_sandbox

# Asset serviti via HTTP (modificabili senza ricompilare il WASM)
mkdir -p wasm-dist/assets
cp -r assets/shaders wasm-dist/assets/
cp assets/preset.json wasm-dist/assets/preset.json
# index.html contiene il glue JS (fetch preset, badge versione): copiarlo
# esplicitamente per non dipendere dal comportamento di wasm-pack
cp index.html wasm-dist/index.html

echo "Build OK: wasm-dist aggiornato (wasm + assets + index.html)"
