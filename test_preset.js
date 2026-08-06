// Verifica end-to-end del preset esterno: carica la pagina, attende l'init,
// legge debug_state() e stampa il conteggio corpi + console.
// Uso: node test_preset.js [url] [wait_ms] [label]
const { chromium } = require('/home/ubuntu/.npm-global/lib/node_modules/playwright');

const url = process.argv[2] || 'http://localhost:8081';
const wait = parseInt(process.argv[3] || '10000', 10);
const label = process.argv[4] || 'test';

(async () => {
  const browser = await chromium.launch({ headless: true, args: ['--no-sandbox'] });
  const page = await browser.newPage();
  const consoleLog = [];
  page.on('console', (msg) => {
    const t = msg.type();
    if (t === 'error' || t === 'warning' || t === 'log') {
      consoleLog.push(`[${t}] ${msg.text().substring(0, 300)}`);
    }
  });
  page.on('pageerror', (err) => consoleLog.push(`[PAGEERROR] ${err.message.substring(0, 300)}`));
  page.on('requestfailed', (req) => consoleLog.push(`[REQFAIL] ${req.url()}`));

  await page.goto(url, { timeout: 30000, waitUntil: 'networkidle' });
  await page.waitForTimeout(wait);

  let state = null;
  try {
    state = await page.evaluate(() => window.__sandbox && window.__sandbox.debug_state());
  } catch (e) {
    state = 'ERR ' + e.message;
  }
  let bodies = 'n/a';
  if (state && state !== 'ERR') {
    try {
      const parsed = JSON.parse(state);
      bodies = parsed.bodies ? parsed.bodies.length : '?';
      consoleLog.push(`[state] tool=${parsed.tool} paused=${parsed.paused} bodies=${parsed.bodies.length} last_system=${parsed.last_system} frame=${parsed.frame}`);
    } catch (e) { /* raw */ }
  }
  const badge = await page.textContent('#version-badge').catch(() => 'n/a');
  console.log(`=== [${label}] BADGE: ${badge}`);
  console.log(`=== [${label}] BODIES: ${bodies}`);
  console.log(`=== [${label}] CONSOLE (${consoleLog.length}) ===`);
  for (const l of consoleLog) console.log(l);
  await browser.close();
})().catch((e) => {
  console.log('FAIL:', e.message);
  process.exit(1);
});
