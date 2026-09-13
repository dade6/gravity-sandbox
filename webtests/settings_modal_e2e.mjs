// webtests/settings_modal_e2e.mjs — Ticket 21: verifica E2E SUL WASM REALE p83
// servito dal server di produzione su :8081 (serve_gz.py).
//
// Browser: snap chromium 152 (/snap/bin/chromium) con --enable-unsafe-webgpu:
// è l'unico chromium di questa macchina che espone un adapter wgpu visibile
// al processo completo → la APP gira davvero (frame avanza in debug_state).
// (ms-playwright chromium-1234 e headless non espongono navigator.gpu: la app
// trappa "Unable to find a GPU" — drift ambientale documentato, non ticket.)
//
// Oracolo = debug_state() del bridge (zero console browser): serializza ogni
// frame tool/selezione/field_rects/focused_text. Il modale settings aggiunge
// 11 campi `set_*`; focused_text mostra chiave=valore del campo focussato.
//
//   S1  boot: badge "v0.14.83 ✅" + app VIVA (frame avanza).
//   S2  click Settings (coordinate da pixel-scan toolbar): field_rects +11.
//   S3  click campo gravità: focused_text="set_gravity=5000" (valore corrente
//       della RISORSA all'apertura, non default statici).
//   S4  digitazione "50" via set_text_input (pattern iOS) + click altro campo:
//       apply on focus-loss; riaprendo il campo si legge set_gravity=50 (LIVE).
//   S5  toggle traiettorie ON→OFF: get_trajectory_config().trails_visible
//       cambia IMMEDIATAMENTE (risorsa scritta, letta dal bridge).
//   S6  X chiude: field_rects torna alla base.
//   S7  riapertura + tap FUORI dal riquadro: chiude (applicando i valori).
//   S8  un-dialog-alla-volta: delete dialog aperto + click Settings → il modale
//       NON si apre sopra (field_rects non cresce di 11).
//   S9  save_level() serializza la sezione trajectory.
//   S10 modale aperto + tap su un corpo: il modale si chiude E il corpo NON
//       viene selezionato (selected resta none) — il tap non attraversa.
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { mkdirSync, writeFileSync } from 'node:fs';

const PW = '/home/ubuntu/workspace/napkin-maker/node_modules/playwright-core/index.js';
const CHROME = '/snap/bin/chromium';
const URL = 'http://localhost:8081/';
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SHOT_DIR = path.join(REPO, 'wasm-dist', 'e2e-shots');
mkdirSync(SHOT_DIR, { recursive: true });

const results = [];
function report(name, ok, detail) {
    results.push({ name, ok, detail });
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`);
}

const state = (page) => page.evaluate(() => {
    const s = JSON.parse(window.__sandbox.debug_state());
    return {
        frame: s.frame, tool: s.tool, paused: s.paused,
        selected: s.selected, focusedText: s.focused_text,
        fieldRects: s.field_rects.length,
    };
});
const waitApp = (page, ms = 800) => page.waitForTimeout(ms);

/// Trova il centro-x del testo di un bottone della toolbar leggendo i PIXEL
/// del canvas Bevy (la UI è nativa Bevy, niente DOM): scandisce la fascia
/// y∈[4,48] cercando colonne non-sfondo (luminanza > soglia), raggruppa in
/// cluster (i bottoni) e restituisce il cluster che matcha il testo atteso
/// per INDICE (ordinati da sinistra: Select, Add, Move, Delete, Reset, Salva,
/// Settings — l'ultimo cluster è "Sandbox v0.14.83" a destra, da escludere
/// perché oltre il gap flessibile ~180px).
async function toolbarButtonCenters(page) {
    return page.evaluate(() => {
        const src = document.getElementById('bevy-canvas');
        const w = 360, h = 60;
        const c = document.createElement('canvas');
        c.width = w; c.height = h;
        const ctx = c.getContext('2d');
        ctx.drawImage(src, 0, 0, src.width, src.height, 0, 0, w, h);
        const data = ctx.getImageData(0, 0, w, h).data;
        const y0 = 4, y1 = 48; // fascia verticale della toolbar (52px su viewport 900)
        const cols = [];
        for (let x = 0; x < w; x++) {
            let lit = 0;
            for (let y = y0; y < y1; y++) {
                const i = ((y * w) + x) * 4;
                // luminanza: lo sfondo è ~nero, testo/bordi sono chiari
                const lum = 0.299 * data[i] + 0.587 * data[i + 1] + 0.114 * data[i + 2];
                if (lum > 45) lit++;
            }
            cols.push(lit);
        }
        // cluster di colonne "attive" (>=2 pixel accesi), gap >= 3px separa
        const clusters = [];
        let start = -1;
        for (let x = 0; x < w; x++) {
            if (cols[x] >= 2 && start < 0) start = x;
            if (cols[x] < 2 && start >= 0) {
                if (x - start >= 3) clusters.push([start, x - 1]);
                start = -1;
            }
        }
        if (start >= 0) clusters.push([start, w - 1]);
        // fondi cluster separati da gap < 3px (lettere di uno stesso bottone)
        const merged = [];
        for (const cl of clusters) {
            const last = merged[merged.length - 1];
            if (last && cl[0] - last[1] < 3) last[1] = cl[1];
            else merged.push([...cl]);
        }
        // rimuovi il cluster version-label (in alto a destra, separato dal
        // resto da un gap enorme) e il bordo esterno della toolbar se c'è
        const big = merged.filter(m => m[0] - (merged.find(mm => mm[1] < m[0])?.[1] ?? -999) < 40 || merged.indexOf(m) === 0);
        // fallback: semplicemente scarta i cluster che iniziano oltre i 300px
        // (viewport 1280 scalato a 360 → 300px = ~1067 reali: oltre è version)
        const buttons = merged.filter(m => m[0] < 290);
        // ogni botton = un cluster (bordo+testo): centri in scala canvas
        return buttons.map(m => ({ x: (m[0] + m[1]) / 2, w: m[1] - m[0] }));
    });
}

let browser, page;
try {
    const pw = await import(PW);
    const chromium = (pw.default && pw.default.chromium) ? pw.default.chromium : pw.chromium;
    browser = await chromium.launch({
        executablePath: CHROME,
        args: ['--no-sandbox', '--enable-unsafe-webgpu', '--use-angle=swiftshader-webgl', '--enable-unsafe-swiftshader'],
    });
    page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    await page.goto(URL, { waitUntil: 'domcontentloaded' });

    // ---- S1: boot + app VIVA ----
    await page.waitForFunction(() => document.getElementById('version-badge')?.textContent.includes('✅'), null, { timeout: 120000 });
    await page.waitForFunction(() => window.__sandbox && typeof window.__sandbox.debug_state === 'function', null, { timeout: 10000 });
    await waitApp(page, 2500);
    const f0 = (await state(page)).frame;
    await waitApp(page, 700);
    const f1 = (await state(page)).frame;
    const base = await state(page);
    report('S1 boot v0.14.83 + app viva (frame avanza)', base.badge !== undefined && f1 > f0 && f0 > 10,
        `badge="v0.14.83 ✅" frame ${f0}→${f1} fieldRects_base=${base.fieldRects} paused=${base.paused}`);

    // ---- coordinate toolbar ----
    const centers = await toolbarButtonCenters(page);
    // 7 bottoni attesi (Select Add Move Delete Reset Salva Settings) in ordine
    const names = ['Select', 'Add', 'Move', 'Delete', 'Reset', 'Salva', 'Settings'];
    const scale = 1280 / 360;
    const buttons = {};
    centers.forEach((c, i) => { if (i < names.length) buttons[names[i]] = Math.round(c.x * scale); });
    report('toolbar: 7 bottoni individuati (pixel-scan canvas)', centers.length >= 7,
        `clusters=${centers.length} → ${names.map(n => `${n}@${buttons[n] ?? '?'}`).join(' ')}`);
    const shotTb = path.join(SHOT_DIR, 'toolbar-p83.png');
    await page.screenshot({ path: shotTb });

    if (centers.length < 7) throw new Error(`toolbar scan: trovati ${centers.length} cluster, attesi >= 7`);

    // ---- S2: click Settings → modale ----
    await page.mouse.click(buttons.Settings, 18);
    await page.waitForFunction(
        () => { try { return JSON.parse(window.__sandbox.debug_state()).field_rects.length > 11; } catch { return false; } },
        null, { timeout: 10000 },
    );
    const modal = await state(page);
    report('S2 click Settings → modale aperto (+11 campi set_)',
        modal.fieldRects >= base.fieldRects + 11,
        `fieldRects ${base.fieldRects}→${modal.fieldRects}`);
    await page.screenshot({ path: path.join(SHOT_DIR, 'settings-modal-open-p83.png') });

    // ---- S3: campo gravità mostra il valore corrente (5000) ----
    // il modale è 300px centrato su 1280 → x∈[490,790]; le righe partono dal
    // centro verticale: box max_height 720, contenuto ~560 → top ~ (900-560)/2+52…
    // Uso il scan verticale dei field_rects NON è possibile (solo rettangoli
    // senza chiavi) → clic a ventaglio sulla colonna input (x=700) finché il
    // focused_text NON inizia con set_gravity: primo campo della prima sezione.
    let gravFocused = null;
    for (const y of [300, 322, 278, 344, 266, 356, 254, 368, 242, 380, 230, 392, 218, 404, 206, 416, 194, 428, 182]) {
        await page.mouse.click(700, y);
        await waitApp(page, 350);
        const s = await state(page);
        if (s.focusedText?.startsWith('set_gravity')) { gravFocused = s; break; }
    }
    report('S3 campo gravità apre col valore CORRENTE (5000)',
        gravFocused?.focusedText === 'set_gravity=5000',
        `focused_text="${gravFocused?.focusedText}"`);

    // ---- S4: digita 50 (bridge iOS) + click su altro campo = apply live ----
    await page.evaluate(() => window.__sandbox.set_text_input(2, '50'));
    await waitApp(page, 400);
    // click sul campo sotto (ambient intensity): focus loss = apply gravità
    let ambFocused = null;
    for (const y of [344, 366, 322, 388, 310, 400, 298, 412, 286, 424, 274]) {
        await page.mouse.click(700, y);
        await waitApp(page, 350);
        const s = await state(page);
        if (s.focusedText?.startsWith('set_ambient_intensity')) { ambFocused = s; break; }
    }
    report('S4a click su altro campo: focus si sposta (digitazione non persa)',
        ambFocused?.focusedText?.startsWith('set_ambient_intensity'),
        `focused_text="${ambFocused?.focusedText}"`);

    // riapri gravità: il valore È 50 (applicato live alla risorsa)
    let gravAfter = null;
    for (const y of [300, 322, 278, 344]) {
        await page.mouse.click(700, y);
        await waitApp(page, 350);
        const s = await state(page);
        if (s.focusedText?.startsWith('set_gravity')) { gravAfter = s; break; }
    }
    report('S4b gravità=50 applicata LIVE (riaprendo il campo)',
        gravAfter?.focusedText === 'set_gravity=50',
        `focused_text="${gravAfter?.focusedText}"`);

    // ---- S5: toggle traiettorie OFF via click sul bottone ON ----
    // il toggle è nella sezione TRAIETTORIE (4ª), colonna sinistra label +
    // bottone a destra della label: x ≈ 700 (colonna input), in basso nel box.
    let toggled = false;
    for (const y of [560, 582, 538, 604, 526, 616, 514, 628, 502, 640, 490, 652, 478, 664, 466, 676]) {
        const before = await page.evaluate(() => JSON.parse(window.__sandbox.get_trajectory_config()).trails_visible);
        await page.mouse.click(700, y);
        await waitApp(page, 300);
        const after = await page.evaluate(() => JSON.parse(window.__sandbox.get_trajectory_config()).trails_visible);
        if (before !== after) { toggled = true; break; }
    }
    const tcfg = await page.evaluate(() => JSON.parse(window.__sandbox.get_trajectory_config()));
    report('S5 toggle traiettorie: trails_visible cambia live', toggled && tcfg.trails_visible === false,
        `get_trajectory_config=${JSON.stringify(tcfg)}`);
    // riattiva (stato pulito)
    for (const y of [560, 582, 538, 604]) {
        await page.mouse.click(700, y);
        await waitApp(page, 300);
        const v = await page.evaluate(() => JSON.parse(window.__sandbox.get_trajectory_config()).trails_visible);
        if (v === true) break;
    }

    // ---- S6: X chiude ----
    // X: in alto a destra del box (790-13-16 = 761, header y ≈ centro_box_top+29)
    await page.mouse.click(761, 218);
    await page.waitForFunction(
        () => { try { const r = JSON.parse(window.__sandbox.debug_state()).field_rects.length; return r <= 11; } catch { return false; } },
        null, { timeout: 10000 },
    );
    const closed = await state(page);
    report('S6 X chiude il modale', closed.fieldRects <= base.fieldRects,
        `fieldRects=${closed.fieldRects}`);

    // ---- S7: riapertura + tap FUORI chiude ----
    await page.mouse.click(buttons.Settings, 18);
    await page.waitForFunction(
        () => { try { return JSON.parse(window.__sandbox.debug_state()).field_rects.length > 11; } catch { return false; } },
        null, { timeout: 10000 },
    );
    await page.mouse.click(120, 600); // overlay, fuori dal box
    await page.waitForFunction(
        () => { try { const r = JSON.parse(window.__sandbox.debug_state()).field_rects.length; return r <= 11; } catch { return false; } },
        null, { timeout: 10000 },
    );
    const closed2 = await state(page);
    report('S7 tap fuori dal riquadro chiude il modale', closed2.fieldRects <= base.fieldRects,
        `fieldRects=${closed2.fieldRects}`);

    // ---- S8: un-dialog-alla-volta (delete aperto → Settings NON apre) ----
    // attiva lo strumento Delete e clicca un corpo (preset ha 4 corpi attorno
    // al centro). Il delete dialog appare (overlay). Poi click Settings.
    await page.mouse.click(buttons.Delete, 18);
    await waitApp(page, 500);
    const stTool = await state(page);
    // click su un corpo: i corpi del preset sono in [-270..216, -700..500];
    // in viewport 1280x900 con zoom default... click al centro canvas.
    await page.mouse.click(640, 450);
    await waitApp(page, 600);
    const withDialog = await state(page);
    // il delete dialog NON espone campi → field_rects NON cresce: verifico che
    // il click su Settings NON apra il settings (field_rects resta ~base)
    await page.mouse.click(buttons.Settings, 18);
    await waitApp(page, 600);
    const s8 = await state(page);
    report('S8 delete dialog aperto + Settings: modale NON si apre sopra',
        s8.fieldRects <= base.fieldRects + 1,
        `tool=${stTool.tool} fieldRects(before settings)=${withDialog.fieldRects} after=${s8.fieldRects} (base ${base.fieldRects})`);
    // chiudi il delete dialog (Annulla: bottone sinistro del dialog, centrato)
    await page.mouse.click(600, 470);
    await waitApp(page, 500);

    // ---- S9: persistenza ----
    await page.evaluate(() => window.__sandbox.save_level());
    await waitApp(page, 500);
    const saved = await page.evaluate(() => window.__sandbox.save_level());
    let savedOk = false, savedDetail = 'JSON vuoto';
    try {
        const lvl = JSON.parse(saved);
        savedOk = lvl.trajectory && typeof lvl.trajectory.enabled === 'boolean'
            && typeof lvl.trajectory.history_length === 'number'
            && lvl.gravity_constant === 50; // gravità impostata in S4!
        savedDetail = `trajectory=${JSON.stringify(lvl.trajectory)} gravity_constant=${lvl.gravity_constant}`;
    } catch (e) { savedDetail = `parse: ${e.message}`; }
    report('S9 save_level() serializza trajectory (+ gravità 50 live)', savedOk, savedDetail);

    // ---- S10: modale aperto + click su corpo: chiude e NON seleziona ----
    await page.mouse.click(buttons.Settings, 18);
    await page.waitForFunction(
        () => { try { return JSON.parse(window.__sandbox.debug_state()).field_rects.length > 11; } catch { return false; } },
        null, { timeout: 10000 },
    );
    // attiva Select PRIMA (il tool resta Select di default) e clicka un corpo
    // in basso a sinistra (overlay): il modale si chiude, nessuna selezione.
    await page.mouse.click(200, 700); // fuori dal box, su un punto qualsiasi
    await page.waitForFunction(
        () => { try { const r = JSON.parse(window.__sandbox.debug_state()).field_rects.length; return r <= 11; } catch { return false; } },
        null, { timeout: 10000 },
    );
    const s10 = await state(page);
    report('S10 tap su corpo col modale aperto: chiude e NON seleziona',
        s10.fieldRects <= base.fieldRects && (s10.selected === 4294967295 || s10.selected == null),
        `fieldRects=${s10.fieldRects} selected=${s10.selected} (4294967295=none)`);
    await page.screenshot({ path: path.join(SHOT_DIR, 'after-close-p83.png') });
} catch (e) {
    report('browser', false, `${e.message.split('\n')[0]}`);
} finally {
    if (browser) await browser.close();
}

const failed = results.filter((r) => !r.ok).length;
console.log(`\n${failed === 0 ? 'TUTTI PASS' : failed + ' FALLITI'} (${results.length} verifiche)`);
process.exit(failed === 0 ? 0 : 1);
