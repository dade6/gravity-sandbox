// Sonda numerica: il modale Settings si apre al click?
// Oracolo = debug_state() (zero console browser). Nessuna vision AI.
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const PW = '/home/ubuntu/workspace/napkin-maker/node_modules/playwright-core/index.js';
const CHROME = '/snap/bin/chromium';
const URL = 'http://localhost:8081/';
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

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

    await page.waitForFunction(() => document.getElementById('version-badge')?.textContent.includes('✅'), null, { timeout: 120000 });
    await page.waitForFunction(() => window.__sandbox && typeof window.__sandbox.debug_state === 'function', null, { timeout: 10000 });
    await page.waitForTimeout(2500);

    const state = (extra) => page.evaluate((e) => {
        const s = JSON.parse(window.__sandbox.debug_state());
        return { frame: s.frame, paused: s.paused, selected: s.selected, fieldRects: s.field_rects.length, focusedText: s.focused_text, ...e };
    }, extra || {});

    const s0 = await state();
    console.log('BASE   :', JSON.stringify(s0));

    // Screenshot full-res del canvas per documentare la toolbar
    const canvas = await page.locator('#bevy-canvas').boundingBox();
    console.log('CANVAS :', JSON.stringify(canvas));
    await page.screenshot({ path: path.join(REPO, 'wasm-dist', 'e2e-shots', 'settings-probe-before.png'), clip: { x: 0, y: 0, width: 1280, height: 100 } });

    // Scan numerico della toolbar a RISOLUZIONE PIENA: fascia y 0..60
    const clusters = await page.evaluate(() => {
        const src = document.getElementById('bevy-canvas');
        const W = 1280, H = 60;
        const c = document.createElement('canvas');
        c.width = W; c.height = H;
        const ctx = c.getContext('2d');
        ctx.drawImage(src, 0, 0, src.width, src.height, 0, 0, W, H);
        const data = ctx.getImageData(0, 0, W, H).data;
        const cols = [];
        for (let x = 0; x < W; x++) {
            let lit = 0;
            for (let y = 2; y < 50; y++) {
                const i = ((y * W) + x) * 4;
                const lum = 0.299 * data[i] + 0.587 * data[i + 1] + 0.114 * data[i + 2];
                if (lum > 45) lit++;
            }
            cols.push(lit);
        }
        const out = [];
        let start = -1;
        for (let x = 0; x < W; x++) {
            if (cols[x] >= 2 && start < 0) start = x;
            if (cols[x] < 2 && start >= 0) {
                if (x - start >= 3) out.push([start, x - 1]);
                start = -1;
            }
        }
        if (start >= 0) out.push([start, W - 1]);
        // fondi cluster con gap < 8px (lettere/bordi di uno stesso bottone)
        const merged = [];
        for (const cl of out) {
            const last = merged[merged.length - 1];
            if (last && cl[0] - last[1] < 8) last[1] = cl[1];
            else merged.push([...cl]);
        }
        return merged;
    });
    console.log('CLUSTERS toolbar:', JSON.stringify(clusters));

    // Attesi (ordine sinistra->destra): Select Add Move Delete Reset Salva Settings
    const names = ['Select', 'Add', 'Move', 'Delete', 'Reset', 'Salva', 'Settings'];
    const buttons = {};
    clusters.slice(0, names.length).forEach((m, i) => {
        buttons[names[i]] = { x: Math.round((m[0] + m[1]) / 2), x0: m[0], x1: m[1] };
    });
    console.log('BOTTONI:', JSON.stringify(buttons));

    if (!buttons.Settings) {
        console.log('>>> SETTINGS NON TROVATO nella toolbar');
    } else {
        // Click sul centro del bottone Settings (y = centro fascia toolbar ~26)
        const sx = buttons.Settings.x, sy = 26;
        console.log(`CLICK Settings @ (${sx}, ${sy})`);
        await page.mouse.click(sx, sy);
        await page.waitForTimeout(1200);
        const s1 = await state({ after: 'click-settings' });
        console.log('DOPO CLICK:', JSON.stringify(s1));
        await page.screenshot({ path: path.join(REPO, 'wasm-dist', 'e2e-shots', 'settings-probe-after.png') });

        const delta = s1.fieldRects - s0.fieldRects;
        console.log(`VERDETTO: field_rects ${s0.fieldRects} -> ${s1.fieldRects} (delta ${delta})`);
        if (delta >= 10) console.log('>>> MODALE SI APRE (>=10 campi set_* aggiunti)');
        else console.log('>>> MODALE NON SI APRE');
    }
} catch (e) {
    console.error('ERRORE:', e.message);
} finally {
    if (browser) await browser.close();
}
