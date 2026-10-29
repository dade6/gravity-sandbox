"""Estrae TUTTI gli script inline da index.html e li valida con `node --check`.

Perche' esiste: il 2026-10-29 ho introdotto un `return;` al livello top di uno
script (fuori da una funzione) e il browser ha rifiutato l'INTERO file con
"Return statements are only valid inside functions" -> pagina morta.

Trabocchetto gia' pagato: la prima versione di questo script cercava solo
`<script>` senza attributi, ma il blocco incriminato stava in
`<script type="module">` -> il gate validava i blocchi innocui e diceva OK su
un file rotto. Ora prende OGNI `<script ...>` inline.

Uso:
    python3 scripts/check_index_js.py [index.html]
exit 0 = sintassi valida per node --check.
"""
import re
import subprocess
import sys
import glob
import os

path = sys.argv[1] if len(sys.argv) > 1 else 'index.html'
src = open(path, encoding='utf-8').read()

# Ogni <script ...> inline (con o senza attributi), esclusi quelli con src=
blocks = []
for m in re.finditer(r'<script([^>]*)>(.*?)</script>', src, re.S):
    attrs, body = m.group(1), m.group(2)
    if 'src=' in attrs:
        continue
    is_module = 'module' in attrs
    blocks.append((is_module, body))

if not blocks:
    print('NESSUN blocco script inline trovato: controllo inutile, esco con errore')
    sys.exit(2)

node = os.environ.get('NODE_BIN')
if not node:
    hits = sorted(glob.glob(os.path.expanduser('~/.hermes/tools/node-*/bin/node')))
    node = hits[-1] if hits else 'node'

fail = 0
for i, (is_module, body) in enumerate(blocks):
    kind = 'module' if is_module else 'classic'
    tmp = f'/tmp/_idx_script_{i}.{"mjs" if is_module else "js"}'
    with open(tmp, 'w', encoding='utf-8') as f:
        f.write(body)
    res = subprocess.run([node, '--check', tmp], capture_output=True, text=True)
    if res.returncode != 0:
        fail += 1
        print(f'❌ blocco {i} ({kind}) NON valido:')
        print((res.stderr or '').strip()[:600])
    else:
        print(f'✓ blocco {i} ({kind}) sintassi OK')

print(f'\n{len(blocks)} blocchi controllati, {fail} non validi')
sys.exit(1 if fail else 0)
