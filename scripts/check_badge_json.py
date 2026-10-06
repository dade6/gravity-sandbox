#!/usr/bin/env python3
"""Valida OFFLINE il JSON del badge costruito da `debug_state_snapshot`.

Perche': il badge WebUI mostra "🛑 DEAD" quando `JSON.parse` fallisce, ma non
dice *perche'*. Un `format!` con placeholder posizionali e' fragile: basta che
un argomento finisca in una posizione sbagliata (es. una stringa in un campo
numerico) per produrre JSON invalido. Questo script ricostruisce il JSON dal
letterale in `src/lib.rs` (sostituendo i placeholder con un valore fittizio)
e lo valida con `json.loads`, senza ricompilare il WASM.

Uso:  python3 scripts/check_badge_json.py [percorso/src/lib.rs]

Limite noto: valida la STRUTTURA del letterale (graffe, virgolette, escape),
non l'ordine degli argomenti rispetto ai placeholder. Per quello vale la
guardia `debug_assertions` in `debug_state_snapshot` (v0.14.120), che sostituisce
un payload invalido con `{"json_error": "..."}` invece di lasciare il badge in DEAD.
"""
import json
import re
import sys

path = sys.argv[1] if len(sys.argv) > 1 else "src/lib.rs"
src = open(path, encoding="utf-8").read()

lines = src.splitlines()
lit = None
for line in lines:
    if 'r#"{' in line and '"last_system"' in line:
        lit = line.strip()
        break
if lit is None:
    print("✗ letterale JSON del badge non trovato in", path, file=sys.stderr)
    sys.exit(2)

# togli il prefisso r#" e il suffisso "#,
body = lit
body = body[body.index('r#"') + 3:]
for suffix in ('"#,', '",'):
    if body.endswith(suffix):
        body = body[: -len(suffix)]
        break

placeholders = len(re.findall(r"\{:\.[0-9]\}", body)) + len(re.findall(r"\{\}", body))
rebuilt = body.replace("{:.0}", "0").replace("{}", "0")
rebuilt = rebuilt.replace("{{", "{").replace("}}", "}")

print(f"letterale: {len(body)} char, {placeholders} placeholder")
try:
    obj = json.loads(rebuilt)
except Exception as e:  # noqa: BLE001
    print("✗ JSON INVALIDO:", e)
    pos = getattr(e, "pos", None)
    if pos is not None:
        print("   contesto:", repr(rebuilt[max(0, pos - 120): pos + 120]))
    sys.exit(1)

print("✓ JSON valido")
if "firefly" in obj:
    print("   chiavi firefly:", sorted(obj["firefly"].keys()))
print("   chiavi top-level:", sorted(obj.keys()))
