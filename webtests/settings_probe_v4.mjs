// Sonda v4: screenshot full-page + stato completo al freeze (frame 98).
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
    await page.waitForTimeout(4000);
    await page.screenshot({ path: path.join(REPO, 'wasm-dist', 'e2e-shots', 'probe-v4-full.png') });
    const s = await page.evaluate(() => {
        const d = JSON.parse(window.__sandbox.debug_state());
        return { frame: d.frame, bodies: d.bodies?.length, last_system: d.last_system, firefly: d.firefly, paused: d.paused };
    });
    console.log('STATE@freeze:', JSON.stringify(s));
    // dettagli canvas: dimensioni reali del backing store
    const dims = await page.evaluate(() => {
        const c = document.getElementById('bevy-canvas');
        return { css: [c.clientWidth, c.clientHeight], buf: [c.width, c.height] };
    });
    console.log('CANVAS DIMS:', JSON.stringify(dims));
} catch (e) {
    console.error('ERRORE:', e.message);
} finally {
    if (browser) await browser.close();
}
