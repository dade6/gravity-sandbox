"""Modello CPU mirato al bordo di Beta, non un renderer Bevy."""

import argparse
from pathlib import Path

import numpy as np
from PIL import Image

SIZE = 128
RADIUS = 20.0
ZOOM = 15.8
FRAME = 680


def bilineare(tex, u, v):
    """Campionamento bilineare con clamp ai bordi del quad."""
    x, y = u * SIZE - .5, v * SIZE - .5
    i, j = np.floor(x).astype(int), np.floor(y).astype(int)
    fx, fy = (x - i)[..., None], (y - j)[..., None]

    def texel(a, b):
        return tex[np.clip(b, 0, SIZE - 1), np.clip(a, 0, SIZE - 1)]

    return ((texel(i, j) * (1 - fx) + texel(i + 1, j) * fx) * (1 - fy)
            + (texel(i, j + 1) * (1 - fx) + texel(i + 1, j + 1) * fx) * fy)


def cono_ombra(x, y, asse):
    """Cono V2, senza rampe aggiuntive e lontano dalla punta dell'umbra."""
    t = -(x * asse[0] + y * asse[1])
    b = np.abs(-x * asse[1] + y * asse[0])
    r = RADIUS + 1.5 * 2 * RADIUS / SIZE + .01 * RADIUS
    umbra = np.maximum(r - t * (45 - r) / 400, 0)
    esterno = r + t * r / 400
    frazione = np.clip((b - umbra) / np.maximum(esterno - umbra, .0001), 0, 1)
    return np.where((t < 0) | (b >= esterno), 0, (1 - frazione)**.25)


def render(disco, normali, angolo):
    a = np.radians(angolo)
    asse = np.array([np.cos(a), np.sin(a)])
    riga = (np.arange(FRAME) + .5 - FRAME / 2) / ZOOM
    x, y = np.meshgrid(riga, -riga)
    u, v = (x + RADIUS) / (2 * RADIUS), (RADIUS - y) / (2 * RADIUS)
    nel_quad = (u >= 0) & (u <= 1) & (v >= 0) & (v <= 1)
    copertura = np.where(nel_quad, bilineare(disco, u, v)[..., 3], 0)
    normal = np.where(nel_quad[..., None], bilineare(normali, u, v), 0)
    scrive = copertura >= .004

    # sprite.wgsl: annulla la premoltiplicazione della texture campionata.
    alpha = normal[..., 3:4]
    rgb = np.where(alpha > 0, normal[..., :3] / np.maximum(alpha, .0001), normal[..., :3])
    target_alpha = np.where(scrive, copertura, 0)[..., None]
    target_rgb = rgb * target_alpha  # ALPHA_BLENDING su target vuoto

    # create_lightmap.wgsl: annulla il blending, poi illumina in TopDownY.
    rgb = target_rgb / np.maximum(target_alpha, .0001)
    n = rgb * 2 - 1
    n = .8 * n / np.maximum(np.linalg.norm(n, axis=-1, keepdims=True), 1e-9)
    # Lo stencil porta la coordinata Y del centro dello sprite.
    luce_x, luce_y = 400 * asse[0] - x, -400 * asse[1]
    normale_luce = np.maximum(0, (n[..., 0] * luce_x + n[..., 1] * luce_y)
                              / np.hypot(luce_x, luce_y))
    superficie = scrive & np.any(rgb != 0, axis=-1)
    normale_luce = np.where(superficie, np.where(rgb[..., 2] == 0, 0, normale_luce), 1)
    shadow = 1 - cono_ombra(x, y, asse)
    luce_superficie = np.where(scrive, 3 * normale_luce, shadow)
    f = target_alpha[..., 0]
    bordo = (f > 0) & (f < 1)
    rapporto = np.where(luce_superficie < shadow, .15, 1)
    peso = f / np.maximum(f + (1 - f) * rapporto, 1e-9)
    lightmap = np.where(bordo, shadow + peso * (luce_superficie - shadow), luce_superficie)

    # apply_lightmap.wgsl: modula il colore già composto per la copertura.
    scena = (copertura[..., None] * np.array([.8, .4, .2])
             + (1 - copertura[..., None]) * .025)
    out = np.clip(scena * np.maximum(lightmap[..., None] * [1, .9, .3], .03), 0, 1)
    return out, copertura, x, y, asse


def misura(out, copertura, x, y, asse):
    r = np.hypot(x, y)
    scala = (r - 4 * RADIUS / SIZE) / np.maximum(r, 1e-6)
    ix = np.clip((FRAME / 2 + x * scala * ZOOM).astype(int), 0, FRAME - 1)
    iy = np.clip((FRAME / 2 - y * scala * ZOOM).astype(int), 0, FRAME - 1)
    eccesso = out[..., 0] - out[iy, ix, 0]
    asse_r = (x * asse[0] + y * asse[1]) / RADIUS
    lato = -x * asse[1] + y * asse[0]
    bordo = (copertura >= .10) & (copertura <= .85)
    settore = bordo & (asse_r >= -.10) & (asse_r < .01)
    return [float(np.quantile(eccesso[settore & (lato * segno > 0)], .9))
            for segno in (-1, 1)]


def leggi(path):
    return np.fromfile(path, dtype=np.uint8).reshape(SIZE, SIZE, 4).astype(np.float32) / 255


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("texture", type=Path, help="directory con i due buffer RGBA8")
    parser.add_argument("--atteso", required=True, choices=("v217", "corretto"))
    parser.add_argument("--immagine", type=Path, help="salva il rendering CPU alto-sx")
    args = parser.parse_args()
    disco, normali = (leggi(args.texture / nome) for nome in ("disco.rgba", "normali.rgba"))
    errori = []
    for quadrante, angolo in (("alto-sx", -33), ("alto-dx", -147)):
        scena = render(disco, normali, angolo)
        valori = misura(*scena)
        print(f"Beta {quadrante}: rosso lineare bordo − interno (p90) "
              f"{valori[0]:+.4f}, {valori[1]:+.4f}")
        if quadrante == "alto-sx" and args.immagine:
            Image.fromarray(np.rint(scena[0] ** (1 / 2.2) * 255).astype(np.uint8)).save(args.immagine)
        if quadrante == "alto-sx" and args.atteso == "v217":
            ok = min(valori) > .10
        else:
            ok = max(valori) < .02
        if not ok:
            errori.append(quadrante)
    if errori:
        raise SystemExit(f"Profilo {args.atteso} non rispettato: {', '.join(errori)}")


if __name__ == "__main__":
    main()
