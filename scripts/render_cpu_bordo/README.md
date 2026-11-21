# Test di rendering su CPU per il bordo di Beta

Verifica i due punti in cui il confine tra luce e ombra incontra la
silhouette. Usa i byte RGBA8 delle texture generate dal gioco e riproduce in
NumPy i passaggi rilevanti di `sprite.wgsl`, `create_lightmap.wgsl` e
`apply_lightmap.wgsl`. Non avvia Bevy, non compila gli shader e non richiede
una GPU. La scena sintetica ha un pianeta, una stella e un fondo costante;
non riproduce tone mapping, precisione del target GPU o tutto il render graph.

Dalla radice del repository, con Rust, Python 3, NumPy e Pillow:

```sh
python3 -m venv /tmp/gs-render-cpu-venv
. /tmp/gs-render-cpu-venv/bin/activate
python -m pip install -r scripts/render_cpu_bordo/requirements.txt
export RENDER_CPU_DIR="$(mktemp -d)"
# Se il target-dir del repository non è scrivibile, impostare CARGO_TARGET_DIR.
cargo test --lib esporta_texture_render_cpu_bordo -- --ignored
python scripts/render_cpu_bordo/verifica.py "$RENDER_CPU_DIR" --atteso corretto --immagine /tmp/bordo-beta.png
```

`--atteso v217` caratterizza il difetto della build originale: filo luminoso
su entrambi gli estremi con Beta in alto a sinistra, assente nella scena
speculare in alto a destra. `--atteso corretto` richiede che il filo non
compaia in nessuno dei due quadranti. In caso di profilo inatteso lo script
esce con un errore. `--immagine` salva, facoltativamente, il risultato
simulato del quadrante in alto a sinistra.

La misura è il 90º percentile dell'eccesso del canale rosso in spazio
lineare sui pixel antialiasati, rispetto a circa due texel più all'interno
lungo lo stesso raggio. Non è una misura di luminanza percettiva né una
comparazione con lo screenshot di Safari.
