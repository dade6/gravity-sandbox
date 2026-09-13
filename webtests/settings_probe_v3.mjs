// Sonda v3: l'app muore a frame 98? Cattura panic + errori + freeze frame.
// Zero console browser manuale: raccoglie tutto via DOM/bridge.
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const PW = '/home/ubuntu/workspace/napkin-maker/node_modules/playwright-core/index.js';
const CHROME = '/snap/bin/chromium';
const URL = 'http://localhost:8081/';

let browser, page;
const errors = [];
try {
    const pw = await import(PW);
    const chromium = (pw.default && pw.default.chromium) ? pw.default.chromium : pw.chromium;
    browser = await chromium.launch({
        executablePath: CHROME,
        args: ['--no-sandbox', '--enable-unsafe-webgpu', '--use-angle=swiftshader-webgl', '--enable-unsafe-swiftshader'],
    });
    page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    page.on('pageerror', (e) => errors.push('PAGEERROR: ' + e.message));
    page.on('console', (m) => {
        if (m.type() === 'error' || m.type() === 'warning') errors.push(`CONSOLE.${m.type()}: ${m.text().slice(0, 300)}`);
    });
    await page.goto(URL, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => document.getElementById('version-badge')?.textContent.includes('✅'), null, { timeout: 120000 });
    await page.waitForFunction(() => window.__sandbox && typeof window.__sandbox.debug_state === 'function', null, { timeout: 10000 });

    // Poll del frame ogni 500ms per 12s: freeze a 98?
    const frames = [];
    for (let i = 0; i < 24; i++) {
        const f = await page.evaluate(() => {
            try { return JSON.parse(window.__sandbox.debug_state()).frame; } catch { return -1; }
        });
        frames.push(f);
        await page.waitForTimeout(500);
    }
    console.log('FRAMES (12s):', JSON.stringify(frames));

    const lastSys = await page.evaluate(() => {
        try { return JSON.parse(window.__sandbox.debug_state()).last_system; } catch { return '?'; }
    });
    console.log('LAST_SYSTEM:', lastSys);
    const lastErr = await page.evaluate(() => { try { return window.__sandbox.get_last_error(); } catch { return 'n/a'; } });
    console.log('LAST_ERROR(bridge):', String(lastErr).slice(0, 200));

    // Pannello errori DOM
    const errPanel = await page.evaluate(() => {
        const p = document.getElementById('error-text');
        return p ? p.textContent : '(no #error-text)';
    });
    console.log('ERROR PANEL:', String(errPanel).slice(0, 500));

    // Il canvas della UI è nero o renderizzato? Screenshot + statistiche pixel fascia alta
    const stats = await page.evaluate(() => {
        const src = document.getElementById('bevy-canvas');
        const W = 640, H = 110;
        const c = document.createElement('canvas');
        c.width = W; c.height = H;
        const ctx = c.getContext('2d');
        ctx.drawImage(src, 0, 0, src.width, src.height, 0, 0, W, H);
        const data = ctx.getImageData(0, 0, W, H).data;
        let lit = 0, maxLum = 0;
        for (let i = 0; i < data.length; i += 4) {
            const lum = 0.299 * data[i] + 0.587 * data[i + 1] + 0.114 * data[i + 2];
            if (lum > 40) lit++;
            if (lum > maxLum) maxLum = lum;
        }
        return { lit, maxLum, total: data.length / 4 };
    });
    console.log('TOP-STRIP PIXEL: lit=' + stats.lit + '/' + stats.total + ' maxLum=' + stats.maxLum);

    // Campiono anche la fascia HUD in basso per confronto (lì il testo si vede)
    const statsBottom = await page.evaluate(() => {
        const src = document.getElementById('bevy-canvas');
        const c = document.createElement('canvas');
        c.width = 640; c.height = 40;
        const ctx = c.getContext('2d');
        ctx.drawImage(src, 0, src.height - 40, src.width, 40, 0, 0, 640, 40);
        const data = ctx.getImageData(0, 0, 640, 40).data;
        let lit = 0;
        for (let i = 0; i < data.length; i += 4) {
            const lum = 0.299 * data[i] + 0.587 * data[i + 1] + 0.114 * data[i + 2];
            if (lum > 40) lit++;
        }
        return { lit };
    });
    console.log('BOTTOM-STRIP PIXEL: lit=' + statsBottom.lit);

    console.log('ERRORS COLLECTED (' + errors.length + '):');
    errors.slice(0, 10).forEach(e => console.log('  ' + e));
} catch (e) {
    console.error('ERRORE SONDA:', e.message);
} finally {
    if (browser) await browser.close();
}
