// webtests/loading_bar_shots.mjs — screenshot visivi della demo (card
// t_63406e55): fasi download/compile/errore + stato dopo fade. NON è un
// test (nessun criterio): produce solo PNG per verifica umana.
import { spawn } from 'node:child_process';
import { setTimeout as sleep } from 'node:timers/promises';

const PW = '/home/ubuntu/workspace/napkin-maker/node_modules/playwright-core/index.js';
const CHROME = '/home/ubuntu/.cache/ms-playwright/chromium-1234/chrome-linux/chrome';
const PORT = 8185;
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = '/tmp/lb-shots';

const server = spawn('node', ['webtests/loading_bar_server.mjs', String(PORT)], { stdio: ['ignore', 'pipe', 'inherit'] });
server.stdout.on('data', (d) => process.stdout.write('[server] ' + d));
await new Promise((res) => {
    server.stdout.on('data', (d) => { if (String(d).includes('fixture server su')) res(); });
    setTimeout(res, 1500);
});

let browser;
try {
    const pw = await import(PW);
    const chromium = (pw.default && pw.default.chromium) ? pw.default.chromium : pw.chromium;
    browser = await chromium.launch({ executablePath: CHROME, args: ['--no-sandbox', '--use-angle=swiftshader-webgl'] });
    const page = await browser.newPage({ viewport: { width: 480, height: 320 } });
    await page.goto(`${BASE}/loading-bar-demo.html`);

    // 1) fase download (attende la % a metà)
    await page.waitForFunction(() => /Download… \d+%/ .test(document.querySelector('.lb-label')?.textContent || ''), null, { timeout: 15000 });
    await sleep(150);
    await page.screenshot({ path: `${OUT}/1-download.png` });
    console.log('shot 1-download.png:', await page.textContent('.lb-label'));

    // 2) fase compile (indeterminato animato)
    await page.waitForFunction(() => (document.querySelector('.lb-label')?.textContent || '').includes('Compilazione'), null, { timeout: 15000 });
    await sleep(400); // a metà animazione, la colonna si vede in posizioni diverse
    await page.screenshot({ path: `${OUT}/2-compile.png` });
    console.log('shot 2-compile.png:', await page.textContent('.lb-label'));

    // 3) stato ready post-fade (barra invisibile, marker fermo)
    await page.waitForFunction(() => document.querySelector('.lb-root')?.className.includes('lb-done'), null, { timeout: 15000 });
    await sleep(600);
    await page.screenshot({ path: `${OUT}/3-ready-faded.png` });
    console.log('shot 3-ready-faded.png: visibility=', await page.evaluate(() => getComputedStyle(document.querySelector('.lb-root')).visibility));

    // 4) errore + retry
    await page.click('#btn-error');
    await page.waitForFunction(() => (document.querySelector('.lb-label')?.textContent || '').includes('Errore'), null, { timeout: 15000 });
    await page.screenshot({ path: `${OUT}/4-error-retry.png` });
    console.log('shot 4-error-retry.png:', await page.textContent('.lb-label'));
} catch (e) {
    console.error('SHOTS ERROR:', e);
    process.exitCode = 1;
} finally {
    if (browser) await browser.close().catch(() => {});
    fetch(`${BASE}/shutdown`, { method: 'POST' }).catch(() => {});
    await sleep(200);
    server.kill();
}
