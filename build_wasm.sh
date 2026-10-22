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
      wasm-dist/*.br wasm-dist/*.gz

# Build WASM: --no-opt evita il timeout di wasm-opt su binary da decine di MB.
# Target dir condiviso configurato in .cargo/config.toml.
RUSTFLAGS='--cfg getrandom_backend="wasm_js"' \
    wasm-pack build --target web --no-opt --out-dir wasm-dist --out-name gravity_sandbox

WASM=wasm-dist/gravity_sandbox_bg.wasm

# Node serve per brotli (zlib ha BROTLI_PARAM_QUALITY 11, il CLI brotli non
# e' installato). In una shell non interattiva node non e' nel PATH: lo
# cerchiamo anche dove lo installa Hermes.
NODE="$(command -v node 2>/dev/null || true)"
if [ -z "$NODE" ]; then
    NODE="$(ls -d "$HOME"/.hermes/tools/node-*/bin/node 2>/dev/null | head -1)"
fi
if [ -z "$NODE" ]; then
    echo "ERRORE: node non trovato (serve per comprimere in brotli)" >&2
    exit 1
fi

# Strip della sezione custom `name`: rustc la emette anche in release e nel
# nostro build pesava ~55MB su 111MB (meta' del download) senza servire a
# nulla a runtime. Il wasm resta valido: la spec la considera opzionale.
python3 scripts/strip_wasm_name.py "$WASM"

# Precompressione. Ordine di preferenza nel server (precompressed.py):
#   .br -> brotli q11, ~6.6MB sul wasm attuale (il piu' piccolo)
#   .gz -> gzip -9, fallback per i client senza brotli
# Il sidecar .size e' la taglia DECOMPRESSA: il reader del browser emette
# byte decompressi anche quando il server manda il .br/.gz, quindi la
# percentuale della loading bar si calcola su questo valore.
"$NODE" scripts/brotli_compress.js "$WASM" "$WASM.br"
gzip -9 -kf "$WASM"
stat -c %s "$WASM" > "$WASM.size"

echo "wasm: $(stat -c %s "$WASM") byte grezzi, $(stat -c %s "$WASM.gz") gzip, $(stat -c %s "$WASM.br") brotli"

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
# wasm_download.js: loader streaming del .wasm con progresso {loaded,
# total, fraction} per la barra di caricamento (usato dal bootstrap di
# index.html; vedi webtests/ per i test dei criteri di accettazione)
cp wasm_download.js wasm-dist/wasm_download.js
# wasm_runtime.js: eventi del ciclo di vita del caricamento WASM
# (download-start/progress/end, compile-start/end, ready, error) attorno
# alla compilazione REALE — API documentata nell'header del modulo;
# test: webtests/run_compile_tests.mjs + webtests/real_check_compile.mjs
cp wasm_runtime.js wasm-dist/wasm_runtime.js
# loading_bar.js: componente UI barra caricamento a due fasi (download %
# + compilazione indeterminata, errore+retry, fade-out senza layout shift,
# ARIA) montato dal bootstrap di index.html via createLoadingBar e collegato
# agli eventi del loader (integrazione t_8553586d); test:
# webtests/run_loading_bar_tests.mjs + webtests/run_integration_tests.mjs
cp loading_bar.js wasm-dist/loading_bar.js

# Precompressione degli asset di testo sulla critical path: glue JS (166KB),
# loader (wasm_download/wasm_runtime), barra di caricamento e index.html.
# Va DOPO le copie qui sopra, altrimenti comprimerebbe i file vecchi.
for f in wasm-dist/gravity_sandbox.js wasm-dist/wasm_download.js \
         wasm-dist/wasm_runtime.js wasm-dist/loading_bar.js wasm-dist/index.html; do
    [ -f "$f" ] || continue
    gzip -9 -kf "$f"
    "$NODE" scripts/brotli_compress.js "$f" "$f.br" | sed 's/^/  /'
done

echo "Build OK: wasm-dist aggiornato (gravity_sandbox + assets + index.html)"