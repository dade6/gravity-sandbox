// webtests/run_integration_tests_firefox.mjs — CRITERIO 4 del task
// (t_8553586d): stesso percorso di accettazione del driver Chromium ma su
// FIREFOX (build playwright 151). Firefox non ha CDP: la rete lenta si
// simula in pagina con un fetchImpl a chunk lenti (deterministico, stesso
// punto in cui agisce la rete reale) — la % di download crescente viene
// comunque verificata dai barLog/classLog reali del DOM.
//   F1 rete lenta simulata: % crescente + byte + fase Compilazione… vista
//      dal DOM + barra scomparsa + wasm chiamabile.
//   F2 flusso veloce + RICARICA: boot completo due volte, zero retry.
//   F3 NESSUNA REGRESSIONE index.html produzione: badge ✅, 0 pageerror
//      pre-boot, barra faded senza click fantasma.
//   F4 errore fatale: barra rossa + bottone Riprota visibile e funzionante.
import { spawn } from 'node:child_process';
import { copyFileSync, unlinkSync, statSync } from 'node:fs';
import path from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

const PW = '/home/ubuntu/workspace/napkin-maker/node_modules/playwright-core/index.js';
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const PORT = Number(process.env.INTEG_FF_PORT || 8188);
const SHOTS = process.env.INTEG_SHOTS || '/tmp/integ-shots';

const results = [];
function report(name, ok, detail) {
    results.push({ name, ok, detail });
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`);
}

for (const f of ['index.html', 'loading_bar.js', 'wasm_runtime.js', 'wasm_download.js']) {
    copyFileSync(path.join(REPO, f), path.join(REPO, 'wasm-dist', f));
}
copyFileSync(path.join(REPO, 'webtests', 'integration_test_page.html'), path.join(REPO, 'wasm-dist', 'integration_test_page.html'));

{
    const probe = await fetch(`http://127.0.0.1:${PORT}/`, { signal: AbortSignal.timeout(1500) }).then(() => true, () => false);
    if (probe) {
        console.error(`FATAL: porta ${PORT} occupata (orphan server di una run precedente)`);
        process.exit(2);
    }
}

const server = spawn('python3', [path.join(REPO, 'serve_wasm.py'), String(PORT), path.join(REPO, 'wasm-dist')], { stdio: ['ignore', 'pipe', 'inherit'] });
let serverReady = false;
server.on('exit', (code) => { if (!serverReady && code !== null) { console.error(`FATAL: serve_wasm.py morto (exit ${code})`); process.exit(2); } });
await new Promise((res) => {
    server.stdout.on('data', (d) => { if (String(d).includes('Serving')) { serverReady = true; res(); } });
    setTimeout(res, 2000);
});
if (!serverReady) { console.error('FATAL: serve_wasm.py non pronto'); process.exit(2); }
console.log(`[integ-ff] serve_wasm.py su http://127.0.0.1:${PORT}`);

function watchPage(page, tag) {
    page.on('console', (m) => { const t = m.text(); if (!/favicon/.test(t)) console.log(`[${tag} console] ${t.substring(0, 300)}`); });
    page.on('pageerror', (e) => console.log(`[${tag} pageerror] ${String(e && e.message ? e.message : e).substring(0, 500)}`));
}
const pullState = (page) => page.evaluate(() => ({
    boot: window.__bootEvents.map(e => ({ name: e.name, payload: e.payload })),
    barLog: window.__barLog,
    classLog: window.__classLog,
    badge: document.getElementById('version-badge').textContent,
}));

let browser;
try {
    const pw = await import(PW);
    const firefox = (pw.default && pw.default.firefox) ? pw.default.firefox : pw.firefox;
    browser = await firefox.launch({ args: ['-headless'] });

    // ============ F1: rete lenta simulata (fetchImpl a chunk ritardati) ====
    {
        const page = await browser.newPage();
        watchPage(page, 'F1');
        // slowFetch: RETE LENTA iniettata dalla pagina stessa (?slow=1) —
        // Firefox non ha CDP: il fetchImpl riemette il body a chunk
        // ritardati (~3MB/s), cross-browser e deterministico.
        await page.goto(`http://127.0.0.1:${PORT}/integration_test_page.html?slow=1`, { waitUntil: 'domcontentloaded' });
        await page.waitForFunction(() => document.getElementById('version-badge').textContent.includes('boot OK'), null, { timeout: 180000 });
        await sleep(700); // fade-out completo (450ms + margine del timer JS)
        const st = await pullState(page);
        const seq = st.boot.map(e => e.name);
        const dlUpdates = st.barLog.filter(x => x.op === 'update' && x.phase === 'download');
        const fracs = dlUpdates.map(x => x.fraction);
        const intermediate = fracs.filter(f => f > 0 && f < 1);
        report('F1a rete lenta: fase Download con % crescente (fraction 0→1, step intermedi)',
            dlUpdates.length >= 3 && fracs[fracs.length - 1] === 1 && intermediate.length >= 2,
            `update=${dlUpdates.length} frac ${fracs[0]?.toFixed(2)}→${fracs[fracs.length - 1]} intermedi=${intermediate.length}`);
        const goodLabels = dlUpdates.filter(x => /^Download… \d+% · [\d.]+ [KMG]B \/ 51\.4 MB$/.test(x.label));
        report('F1b label download con % e byte (MB / 51.4 MB)', goodLabels.length >= 1,
            `es. "${dlUpdates.find(x => /\d+%/.test(x.label))?.label || 'NESSUNA'}" (${goodLabels.length} esatte)`);
        const fillW = st.classLog.filter(x => x.src === 'fill' && x.fill && x.fill !== '0%').map(x => parseFloat(x.fill));
        const fillInterm = fillW.filter(w => w > 0 && w < 100);
        report('F1c fill width crescente nel DOM reale (0→100%)',
            fillW.length >= 3 && Math.max(...fillW) === 100 && fillInterm.length >= 2,
            `widths=${fillW.slice(0, 5).join(',')}…`);
        const compSeen = st.classLog.filter(x => x.cls && x.cls.includes('lb-indeterminate') && x.label === 'Compilazione…');
        report('F1d fase Compilazione… indeterminata animata (classe vista nel DOM)', compSeen.length >= 1,
            `mutazioni compile=${compSeen.length}`);
        const want = ['download-start', 'download-progress', 'download-end', 'compile-start', 'compile-end', 'ready'];
        const ordered = want.every(n => seq.includes(n)) && want.every((n, i) => i === 0 || seq.indexOf(want[i - 1]) < seq.indexOf(n));
        report('F1e sequenza eventi completa e in ordine', ordered === true,
            `seq=${seq.filter((n, i) => seq.indexOf(n) === i).join('>')}`);
        const gone = await page.evaluate(() => {
            const r = document.querySelector('.lb-root');
            if (!r) return { present: false };
            const cs = getComputedStyle(r);
            return { present: true, vis: cs.visibility, pe: cs.pointerEvents, disp: cs.display, cls: r.className };
        });
        report('F1f barra scomparsa: visibility hidden + pointer-events none',
            gone.present === true && gone.vis === 'hidden' && gone.pe === 'none' && gone.disp !== 'none',
            `vis=${gone.vis} pe=${gone.pe} cls=${gone.cls}`);
        await sleep(2500);
        const alive = await page.evaluate(() => {
            try {
                if (!window.__sandbox || typeof window.__sandbox.debug_state !== 'function') return { callable: false };
                const raw = window.__sandbox.debug_state();
                return { callable: true, rawLen: raw.length, parsed: (() => { try { return !!JSON.parse(raw); } catch (e) { return false; } })() };
            } catch (e) { return { callable: false, err: String(e.message) }; }
        });
        report('F1g simulazione avviata: wasm caricato, istanziato e chiamabile',
            alive.callable === true && alive.rawLen >= 0,
            `callable=${alive.callable} rawLen=${alive.rawLen} parsed=${alive.parsed}`);
        await page.close();
    }

    // ============ F2: flusso veloce + ricarica =============================
    {
        const page = await browser.newPage();
        watchPage(page, 'F2');
        await page.goto(`http://127.0.0.1:${PORT}/integration_test_page.html`, { waitUntil: 'domcontentloaded' });
        await page.waitForFunction(() => document.getElementById('version-badge').textContent.includes('boot OK'), { timeout: 180000 });
        const st = await pullState(page);
        const seq = st.boot.map(e => e.name);
        const names = ['download-start', 'download-progress', 'download-end', 'compile-start', 'compile-end', 'ready'];
        const okOrder = names.every(n => seq.includes(n)) && seq.indexOf('download-start') < seq.indexOf('compile-start') && seq.indexOf('compile-start') < seq.indexOf('ready');
        report('F2a boot veloce locale: sequenza completa e in ordine', okOrder,
            `seq=${seq.filter((n, i) => seq.indexOf(n) === i).join('>')}`);
        report('F2b nessun retry in flusso normale', !seq.includes('retry'),
            `retries=${seq.filter(x => x === 'retry').length}`);
        await page.reload({ waitUntil: 'domcontentloaded' });
        await page.waitForFunction(() => document.getElementById('version-badge').textContent.includes('boot OK'), { timeout: 180000 });
        const st2 = await pullState(page);
        report('F2c ricarica pagina: boot completo di nuovo (nessuna barra bloccata)',
            st2.badge.includes('boot OK') && st2.boot.some(e => e.name === 'ready') && !st2.boot.some(e => e.name === 'retry'),
            `badge="${st2.badge}"`);
        await page.close();
    }

    // ============ F3: NESSUNA REGRESSIONE — index.html produzione ==========
    {
        const page = await browser.newPage();
        watchPage(page, 'F3');
        const pageErrors = [];
        let badgeTime = 0;
        page.on('pageerror', (e) => pageErrors.push({ t: Date.now(), msg: String(e && e.message ? e.message : e) }));
        const t0 = Date.now();
        await page.goto(`http://127.0.0.1:${PORT}/index.html`, { waitUntil: 'domcontentloaded' });
        await page.waitForFunction(() => {
            const v = document.getElementById('version-badge');
            return v && v.textContent.indexOf('✅') >= 0;
        }, { timeout: 180000 });
        badgeTime = Date.now();
        await sleep(3000);
        const barState = await page.evaluate(() => {
            const r = document.querySelector('#ui-overlay .lb-root');
            if (!r) return { present: false };
            const cs = getComputedStyle(r);
            return { present: true, cls: r.className, visibility: cs.visibility, pointerEvents: cs.pointerEvents, display: cs.display };
        });
        const alive = await page.evaluate(() => {
            try {
                if (!window.__sandbox || typeof window.__sandbox.debug_state !== 'function') return { callable: false };
                const raw = window.__sandbox.debug_state();
                return { callable: true, rawLen: raw.length, parsed: (() => { try { return !!JSON.parse(raw); } catch (e) { return false; } })() };
            } catch (e) { return { callable: false, err: String(e.message) }; }
        });
        const preBoot = pageErrors.filter(pe => pe.t < badgeTime);
        report('F3a boot produzione: badge ✅ + 0 pageerror pre-boot + wasm chiamabile',
            barState.present === true && preBoot.length === 0 && alive.callable === true,
            `bar=${barState.present} pre=${preBoot.length} callable=${alive.callable} parsed=${alive.parsed} t=${Date.now() - t0}ms${pageErrors.length ? ' err[' + pageErrors.map(pe => pe.msg).join(' | ').substring(0, 200) + ']' : ''}`);
        report('F3b barra faded: visibility hidden + pointer-events none',
            barState.visibility === 'hidden' && barState.pointerEvents === 'none',
            `vis=${barState.visibility} pe=${barState.pointerEvents} cls=${barState.cls}`);
        await page.close();
    }

    // ============ F4: errore fatale -> barra errore + bottone Riprova =====
    {
        const page = await browser.newPage();
        watchPage(page, 'F4');
        await page.goto(`http://127.0.0.1:${PORT}/integration_test_page.html?wasm=/definitivamente-assente.wasm&size=/definitivamente-assente.size`, { waitUntil: 'domcontentloaded' });
        await page.waitForFunction(() => document.getElementById('version-badge').textContent.includes('boot ERR'), { timeout: 30000 });
        const errState = await page.evaluate(() => {
            const r = document.querySelector('.lb-root');
            const btn = r ? r.querySelector('.lb-retry') : null;
            const row = btn ? btn.parentElement : null;
            return {
                cls: r ? r.className : '',
                label: r ? r.querySelector('.lb-label').textContent : '',
                btnVisible: row ? getComputedStyle(row).display !== 'none' : false,
                btnText: btn ? btn.textContent : '',
            };
        });
        report('F4a errore fatale: barra rossa + messaggio + bottone Riprova visibile',
            errState.cls.includes('lb-error') && errState.cls.includes('lb-retryable') && errState.btnVisible && /Riprova/.test(errState.btnText),
            `cls="${errState.cls}" label="${errState.label}" btn="${errState.btnText}"`);
        await page.evaluate(() => { document.querySelector('.lb-root .lb-retry').click(); });
        await page.waitForFunction(() => window.__barLog.filter(x => x.op === 'error').length >= 2, { timeout: 30000 });
        const after = await pullState(page);
        const errCount = after.barLog.filter(x => x.op === 'error').length;
        report('F4b click Riprova: boot rilanciato e di nuovo in errore (bottone funzionante)',
            errCount === 2 && after.badge.includes('boot ERR'),
            `error-state=${errCount} badge="${after.badge}"`);
        await page.close();
    }
} catch (e) {
    console.error('DRIVER ERROR:', e);
    process.exitCode = 1;
} finally {
    if (browser) await browser.close().catch(() => {});
    server.kill();
    try { unlinkSync(path.join(REPO, 'wasm-dist', 'integration_test_page.html')); } catch (e) {}
    await sleep(200);
}

const failed = results.filter(r => !r.ok).length;
console.log(failed === 0
    ? `\nINTEGRAZIONE FIREFOX COMPLETA — ${results.length}/${results.length} PASS`
    : `\n${failed}/${results.length} TEST FIREFOX FALLITI`);
process.exit(failed === 0 ? 0 : 1);
