// Test console: carica la sandbox in headless e cattura TUTTI i messaggi
// console + errori JS + pageerror. I panic Rust su WASM (es. B0001) sono
// riproducibili in headless perché avvengono PRIMA del render.
// Uso: node test_console.js [url] [wait_ms]
const { chromium } = require('/home/ubuntu/.npm-global/lib/node_modules/playwright');

const url = process.argv[2] || 'http://localhost:8081';
const wait = parseInt(process.argv[3] || '8000', 10);

(async () => {
  const browser = await chromium.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  const consoleLog = [];
  page.on('console', (msg) => {
    const t = msg.type();
    if (t === 'error' || t === 'warning' || t === 'log' || t === 'debug') {
      consoleLog.push(`[${t}] ${msg.text().substring(0, 500)}`);
    }
  });
  page.on('pageerror', (err) => {
    consoleLog.push(`[PAGEERROR] ${err.message.substring(0, 500)}`);
  });
  page.on('requestfailed', (req) => {
    consoleLog.push(`[REQFAIL] ${req.url()} ${req.failure()?.errorText || ''}`);
  });

  await page.goto(url, { timeout: 30000, waitUntil: 'networkidle' });
  await page.waitForTimeout(wait);

  const badge = await page.textContent('#version-badge').catch(() => 'n/a');
  const debug = await page.textContent('#debug-state').catch(() => 'n/a');
  console.log('=== BADGE:', badge);
  console.log('=== DEBUG:', debug.substring(0, 120));
  console.log('=== CONSOLE (' + consoleLog.length + ' msgs) ===');
  for (const l of consoleLog) console.log(l);
  if (consoleLog.length === 0) console.log('(console pulita — nessun errore)');
  await browser.close();
})().catch((e) => {
  console.log('FAIL:', e.message);
  process.exit(1);
});
