// Sonda v6: bringToFront + frames — il freeze era rAF throttling?
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const PW = '/home/ubuntu/workspace/napkin-maker/node_modules/playwright-core/index.js';
const CHROME = '/snap/bin/chromium';

let browser;
try {
    const pw = await import(PW);
    const chromium = (pw.default && pw.default.chromium) ? pw.default.chromium : pw.chromium;
    browser = await chromium.launch({
        executablePath: CHROME,
        args: ['--no-sandbox', '--enable-unsafe-webgpu', '--use-angle=swiftshader-webgl', '--enable-unsafe-swiftshader'],
    });
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    await page.goto('http://localhost:8081/', { waitUntil: 'domcontentloaded' });
    await page.bringToFront();  // <-- LA DIFFERENZA
    await page.waitForFunction(() => document.getElementById('version-badge')?.textContent.includes('✅'), null, { timeout: 120000 });
    await page.waitForFunction(() => window.__sandbox && typeof window.__sandbox.debug_state === 'function', null, { timeout: 10000 });
    await page.waitForTimeout(1500);
    const f0 = await page.evaluate(() => JSON.parse(window.__sandbox.debug_state()).frame);
    await page.waitForTimeout(4000);
    const f1 = await page.evaluate(() => JSON.parse(window.__sandbox.debug_state()).frame);
    console.log(`bringToFront: frame ${f0} -> ${f1}  ${f1 > f0 + 60 ? 'VIVA ✅ (rAF attivo)' : 'FERMA ❌'}`);
    if (f1 > f0 + 60) {
        // App VIVA: ora il test vero — click Settings e verifica modale
        const shot = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'wasm-dist', 'e2e-shots');
        // trova il bottone Settings via pixel-scan full-res
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
            const merged = [];
            for (const cl of out) {
                const last = merged[merged.length - 1];
                if (last && cl[0] - last[1] < 8) last[1] = cl[1];
                else merged.push([...cl]);
            }
            return merged;
        });
        console.log('clusters:', JSON.stringify(clusters.map(m => [m[0], m[1]])));
        const names = ['Select', 'Add', 'Move', 'Delete', 'Reset', 'Salva', 'Settings'];
        const s = clusters.slice(0, 7);
        if (s.length >= 7) {
            const sx = Math.round((s[6][0] + s[6][1]) / 2);
            console.log(`Settings @ x=${sx} (cluster 7/7) — CLICK`);
            const before = await page.evaluate(() => JSON.parse(window.__sandbox.debug_state()).field_rects.length);
            await page.mouse.click(sx, 26);
            await page.waitForTimeout(1000);
            const after = await page.evaluate(() => JSON.parse(window.__sandbox.debug_state()));
            console.log(`field_rects ${before} -> ${after.field_rects.length}, focused=${after.focused_text}`);
            console.log(after.field_rects.length - before >= 10 ? '>>> MODALE APERTO ✅' : '>>> MODALE NON APERTO ❌');
            await page.screenshot({ path: shot + '/settings-modal-v6.png' });
        } else {
            console.log(`cluster trovati: ${s.length} (attesi 7)`);
            await page.screenshot({ path: shot + '/settings-toolbar-v6.png' });
        }
    }
} catch (e) {
    console.error('ERRORE:', e.message);
} finally {
    if (browser) await browser.close();
}
