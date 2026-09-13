// Sonda v5: confronto p80 (old-index) vs p83 (index) — il freeze a frame 98
// è regressione del deploy recente o artefatto headless?
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const PW = '/home/ubuntu/workspace/napkin-maker/node_modules/playwright-core/index.js';
const CHROME = '/snap/bin/chromium';

const CONFIGS = [
    { name: 'p80-old-index', url: 'http://localhost:8081/old-index.html?v=0.14.80' },
    { name: 'p83-new-index', url: 'http://localhost:8081/?v=0.14.83' },
];

let browser;
try {
    const pw = await import(PW);
    const chromium = (pw.default && pw.default.chromium) ? pw.default.chromium : pw.chromium;
    browser = await chromium.launch({
        executablePath: CHROME,
        args: ['--no-sandbox', '--enable-unsafe-webgpu', '--use-angle=swiftshader-webgl', '--enable-unsafe-swiftshader'],
    });
    for (const cfg of CONFIGS) {
        const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
        const errs = [];
        page.on('pageerror', (e) => errs.push('PAGEERROR: ' + e.message.slice(0, 200)));
        page.on('console', (m) => { if (m.type() === 'error') errs.push('C.err: ' + m.text().slice(0, 200)); });
        try {
            await page.goto(cfg.url, { waitUntil: 'domcontentloaded' });
            await page.waitForFunction(() => document.getElementById('version-badge')?.textContent.includes('✅'), null, { timeout: 120000 });
            await page.waitForFunction(() => window.__sandbox && typeof window.__sandbox.debug_state === 'function', null, { timeout: 10000 });
            await page.waitForTimeout(1500);
            const f0 = await page.evaluate(() => JSON.parse(window.__sandbox.debug_state()).frame);
            await page.waitForTimeout(3000);
            const f1 = await page.evaluate(() => JSON.parse(window.__sandbox.debug_state()).frame);
            await page.waitForTimeout(3000);
            const f2 = await page.evaluate(() => JSON.parse(window.__sandbox.debug_state()).frame);
            console.log(`${cfg.name}: frame ${f0} -> ${f1} -> ${f2}  ${f2 > f1 && f1 > f0 ? 'VIVA ✅' : 'CONGELATA ❌'}`);
            if (errs.length) console.log('  errori: ' + errs.slice(0, 3).join(' | '));
        } catch (e) {
            console.log(`${cfg.name}: FALLITO BOOT — ${e.message.slice(0, 150)}`);
            if (errs.length) console.log('  errori: ' + errs.slice(0, 3).join(' | '));
        } finally {
            await page.close();
        }
    }
} catch (e) {
    console.error('ERRORE:', e.message);
} finally {
    if (browser) await browser.close();
}
