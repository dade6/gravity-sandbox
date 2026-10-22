#!/usr/bin/env python3
"""Analizza un log di movimento della sandbox (CSV prodotto da `POST /log`).

Uso:
    python3 scripts/analyze_motion_log.py ~/gravity-logs/motion-*.csv
    python3 scripts/analyze_motion_log.py FILE --pad 3.0     # secondi attorno allo zoom
    python3 scripts/analyze_motion_log.py FILE --event 2     # solo un evento

Cosa cerca: gli scatti del pianeta. Si misura lo scostamento a schermo del
corpo rispetto alla camera — `sx = (body_x - cam_x) / zoom` — e si guardano i
delta per frame:

  * `fermi`  = frame in cui il corpo NON si è mosso affatto (scatto classico
    da quantizzazione: il corpo avanza solo nei tick fisici);
  * `max`    = delta massimo (il "salto");
  * ratio    = max / mediana: > ~3 con molti frame fermi = moto a scatti;
  * `gap`    = distanza Transform↔Position: se è ~0 sempre, l'interpolazione
    non sta producendo nulla (il `ease` dice se lo stato è attivo);
  * `fract`  = overstep_fraction: deve crescere in modo continuo e ripartire
    da 0 a ogni tick.

Le finestre sono centrate sui cambi di zoom (raggruppati entro 1 s), perché è
lì che l'utente vede il difetto.
"""
import argparse
import csv
import glob
import os
import statistics as st
import sys

COLS = [
    "t_ms", "dt_ms", "zoom", "cam_x", "cam_y", "followed", "body_idx",
    "body_x", "body_y", "phys_x", "phys_y", "gap", "fract", "ease", "speed",
]


def load(path):
    rows = []
    with open(path, newline="") as fh:
        reader = csv.DictReader(fh)
        for raw in reader:
            if not raw or raw.get("t_ms") is None:
                continue
            try:
                row = {k: float(raw[k]) for k in COLS if raw.get(k) not in (None, "")}
            except (KeyError, ValueError):
                continue
            rows.append(row)
    rows.sort(key=lambda r: r["t_ms"])
    return rows


def screen_offsets(rows):
    """Scostamento a schermo (px) del corpo rispetto al centro camera."""
    out = []
    for r in rows:
        z = r.get("zoom") or 1.0
        out.append(((r.get("body_x", 0.0) - r.get("cam_x", 0.0)) / z,
                    (r.get("body_y", 0.0) - r.get("cam_y", 0.0)) / z))
    return out


def zoom_events(rows, min_rel=0.005, group_ms=1000.0):
    events = []
    prev = None
    for r in rows:
        z = r.get("zoom")
        if z is None:
            continue
        if prev is not None and prev > 0 and abs(z - prev) / prev > min_rel:
            if not events or r["t_ms"] - events[-1] > group_ms:
                events.append(r["t_ms"])
        prev = z
    return events


def window_stats(rows, offs, t0, t1):
    idx = [i for i, r in enumerate(rows) if t0 <= r["t_ms"] <= t1]
    if len(idx) < 6:
        return None
    steps, frozen, gaps, eases, fracts, dts = [], 0, [], [], [], []
    for a, b in zip(idx, idx[1:]):
        dx = offs[b][0] - offs[a][0]
        dy = offs[b][1] - offs[a][1]
        mag = (dx * dx + dy * dy) ** 0.5
        steps.append(mag)
        if mag == 0.0:
            frozen += 1
        gaps.append(rows[a].get("gap", 0.0))
        eases.append(int(rows[a].get("ease", 0)))
        fracts.append(rows[a].get("fract", 0.0))
        dts.append(rows[a].get("dt_ms", 0.0))
    med = st.median(steps) if steps else 0.0
    mx = max(steps) if steps else 0.0
    p90 = sorted(steps)[int(0.9 * (len(steps) - 1))] if steps else 0.0
    # Continuità della frazione: salti negativi = tick; salti positivi enormi
    # o frazione ferma = qualcosa non avanza.
    dfract = [b - a for a, b in zip(fracts, fracts[1:])]
    # La frazione deve crescere dentro un tick e ripartire da 0: un calo è un
    # tick. Soglia bassa (-0.05) perché con dt ~8.3ms il calo misurato è
    # -0.47, non -1: una soglia a -0.5 non conterebbe NULLA (successo in
    # validazione con dati sintetici).
    tick_drops = sum(1 for d in dfract if d < -0.05)
    frac_stuck = sum(1 for d in dfract if abs(d) < 1e-6)
    span_s = (rows[idx[-1]]["t_ms"] - rows[idx[0]]["t_ms"]) / 1000.0
    expected_ticks = span_s / (1.0 / 64.0)
    return {
        "n": len(idx),
        "med": med, "p90": p90, "max": mx,
        "ratio": (mx / med) if med > 1e-9 else float("inf"),
        "frozen": frozen,
        "frozen_pct": 100.0 * frozen / max(1, len(steps)),
        "gap_med": st.median(gaps) if gaps else 0.0,
        "gap_max": max(gaps) if gaps else 0.0,
        "ease_min": min(eases) if eases else -1,
        "ticks": tick_drops,
        "ticks_expected": expected_ticks,
        "frac_stuck_pct": 100.0 * frac_stuck / max(1, len(dfract)),
        "dt_med": st.median(dts) if dts else 0.0,
        "dt_max": max(dts) if dts else 0.0,
        "steps": steps,
        "t0": rows[idx[0]]["t_ms"],
        "zoom": rows[idx[0]].get("zoom", 0.0),
    }


def sparkline(values, width=60):
    if not values:
        return ""
    lo, hi = min(values), max(values)
    if hi - lo < 1e-9:
        return "▁" * width
    chars = " ▁▂▃▄▅▆▇█"
    out = []
    for i in range(width):
        a = int(i * len(values) / width)
        b = max(a + 1, int((i + 1) * len(values) / width))
        v = sum(values[a:b]) / (b - a)
        k = int((v - lo) / (hi - lo) * (len(chars) - 1))
        out.append(chars[max(0, min(len(chars) - 1, k))])
    return "".join(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("path")
    ap.add_argument("--pad", type=float, default=3.0, help="secondi prima/dopo lo zoom")
    ap.add_argument("--event", type=int, default=None)
    args = ap.parse_args()

    paths = sorted(glob.glob(args.path)) or [args.path]
    for path in paths:
        rows = load(path)
        if not rows:
            print(f"{path}: nessuna riga valida")
            continue
        offs = screen_offsets(rows)
        events = zoom_events(rows)
        dur = (rows[-1]["t_ms"] - rows[0]["t_ms"]) / 1000.0
        print(f"\n=== {os.path.basename(path)} ===")
        print(f"{len(rows)} campioni, {dur:.1f}s, {len(events)} cambi di zoom")
        ease_vals = {int(r.get('ease', 0)) for r in rows}
        print(f"ease visto: {sorted(ease_vals)}  (2=attivo, 1=azzerato, 0=assente)  "
              f"gap mediano globale: {st.median([r.get('gap', 0.0) for r in rows]):.4f}")
        if not events:
            print("nessun cambio di zoom: analizzo l'intero log")
            events = [rows[0]["t_ms"]]
        pad = args.pad * 1000.0
        for n, ev in enumerate(events):
            if args.event is not None and n != args.event:
                continue
            w = window_stats(rows, offs, ev - pad, ev + pad)
            if not w:
                continue
            print(f"\n-- evento {n} a t={ev/1000:.2f}s (zoom {w['zoom']:.3f}) --")
            print(f"   frame={w['n']} dt_med={w['dt_med']:.2f}ms dt_max={w['dt_max']:.1f}ms")
            print(f"   passo px: mediana={w['med']:.3f} p90={w['p90']:.3f} max={w['max']:.3f} "
                  f"ratio={w['ratio']:.2f}")
            print(f"   frame FERMI: {w['frozen']}/{w['n']-1} ({w['frozen_pct']:.1f}%)")
            print(f"   gap: mediana={w['gap_med']:.4f} max={w['gap_max']:.4f}  ease_min={w['ease_min']}")
            print(f"   tick visti={w['ticks']} (attesi ~{w['ticks_expected']:.0f})  "
                  f"frazione ferma={w['frac_stuck_pct']:.1f}%")
            print(f"   passi: {sparkline(w['steps'])}")


if __name__ == "__main__":
    sys.exit(main())
