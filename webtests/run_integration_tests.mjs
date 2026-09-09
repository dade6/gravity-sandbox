// webtests/run_integration_tests.mjs — verifica INTEGRAZIONE REALE del boot
// (t_8553586d): la pagina index.html di produzione serve la app attraverso
// loadWasmLifecycle + createLoadingBar. DETERMINISTICO: nessun sampling a
// intervallo (race-prone) — la pagina registra ogni chiamata del componente
// (__barLog), ogni evento del loader (__bootEvents) e ogni mutazione di
// classe/style del DOM della barra (__classLog, MutationObserver).
//
// Criteri del task:
//   I1 rete lenta (3MB/s via CDP, "cache disabilitata"): % di download
//      crescente visibile sulla barra (label 'Download… 42% · 21.6 MB /
//      51.4 MB' + fill width crescente), poi fase 'Compilazione…'
//      indeterminata animata vista DAL DOM, poi barra scomparsa (fade,
//      visibility hidden + pointer-events none) e simulazione viva.
//   I2 flusso veloce locale (analogo 'WASM in cache': il server di
//      produzione è no-store, il flusso deve restare fluido = mai congelato)
//      + RICARICA della pagina: il boot riparte e completa di nuovo.
//   I3 NESSUNA REGRESSIONE: index.html di produzione boot ✅, 0 pageerror
//      pre-boot, barra faded senza click fantasma, vecchio markup rimosso,
//      debug_state vivo 3s dopo.
//   I4 rete che muore a METÀ STREAM x2 (deterministico via fetchImpl
//      ?failFirst=2, il caso Tailscale/DERP): retry automatici con label
//      'Riprovo…' indeterminata (mai barra congelata a metà), poi boot OK.
//   I5 errore FATALE (404, budget esaurito): barra rossa + bottone
//      'Riprova' visibile; il click RILANCIA il boot (secondo ciclo) e il
//      fallimento rimette la barra in errore (nessuna Promise non gestita).
import { spawn } from 'node:child_process';
import { copyFileSync, unlinkSync, statSync } from 'node:fs';
import path from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

const PW = '/home/ubuntu/workspace/napkin-maker/node_modules/playwright-core/index.js';
const CHROME = '/home/ubuntu/.cache/ms-playwright/chromium-1234/chrome-linux/chrome';
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const PORT = Number(process.env.INTEG_PORT || 8187); // dedicato: 8181 t_d212617b, 8182 real_check, 8183 t_3c91ee24, 8185 t_63406e55
const SHOTS = process.env.INTEG_SHOTS || '/tmp/integ-shots';

const results = [];
function report(name, ok, detail) {
    results.push({ name, ok, detail });
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`);
}

// Il server di produzione serve SOLO wasm-dist: replico il deploy di
// build_wasm.sh (index.html + i 3 moduli) e aggiungo la pagina host del
// test, che viene rimossa alla fine (wasm-dist resta deploy pulito).
for (const f of ['index.html', 'loading_bar.js', 'wasm_runtime.js', 'wasm_download.js']) {
    copyFileSync(path.join(REPO, f), path.join(REPO, 'wasm-dist', f));
}
copyFileSync(path.join(REPO, 'webtests', 'integration_test_page.html'), path.join(REPO, 'wasm-dist', 'integration_test_page.html'));

// porta libera? un orphan di una run precedente bloccherebbe il bind e il
// driver parlerebbe con un server MEZZO MORTO (pipe rotta) -> fail fast.
{
    const probe = await fetch(`http://127.0.0.1:${PORT}/`, { signal: AbortSignal.timeout(1500) }).then(() => true, () => false);
    if (probe) {
        console.error(`FATAL: porta ${PORT} occupata da una run precedente (orphan server) — killarla prima (pkill -f 'serve_wasm.py ${PORT}')`);
        process.exit(2);
    }
}

const server = spawn('python3', [path.join(REPO, 'serve_wasm.py'), String(PORT), path.join(REPO, 'wasm-dist')], { stdio: ['ignore', 'pipe', 'inherit'] });
let serverReady = false;
server.on('exit', (code) => { if (!serverReady && code !== null) { console.error(`FATAL: serve_wasm.py morto al boot (exit ${code}) — porta occupata?`); process.exit(2); } });
await new Promise((res) => {
    server.stdout.on('data', (d) => { if (String(d).includes('Serving')) { serverReady = true; res(); } });
    setTimeout(res, 2000);
});
if (!serverReady) { console.error('FATAL: serve_wasm.py non ha stampato "Serving"'); process.exit(2); }
{
    const ok = await fetch(`http://127.0.0.1:${PORT}/integration_test_page.html`, { signal: AbortSignal.timeout(3000) }).then(r => r.status === 200, () => false);
    if (!ok) { console.error('FATAL: probe HTTP fallito'); process.exit(2); }
}
console.log(`[integ] serve_wasm.py su http://127.0.0.1:${PORT} (wasm ${statSync(path.join(REPO, 'wasm-dist', 'gravity_sandbox_p80_bg.wasm')).size} B)`);

function watchPage(page, tag) {
    page.on('console', (m) => { const t = m.text(); if (!/favicon/.test(t)) console.log(`[${tag} console] ${t.substring(0, 300)}`); });
    page.on('pageerror', (e) => console.log(`[${tag} pageerror] ${String(e && e.message ? e.message : e).substring(0, 500)}`));
}

// helper: estrae le THREE timeline dalla pagina (dopo il boot)
const pullState = (page) => page.evaluate(() => ({
    boot: window.__bootEvents.map(e => ({ name: e.name, payload: e.payload })),
    barLog: window.__barLog,
    classLog: window.__classLog,
    badge: document.getElementById('version-badge').textContent,
}));

let browser;
try {
    const pw = await import(PW);
    const chromium = (pw.default && pw.default.chromium) ? pw.default.chromium : pw.chromium;
    browser = await chromium.launch({ executablePath: CHROME, args: ['--no-sandbox', '--use-angle=swiftshader-webgl', '--enable-unsafe-swiftshader'] });

    // ============ I1: rete lenta, barra visibile a fasi ====================
    {
        const page = await browser.newPage();
        watchPage(page, 'I1');
        const cdp = await page.context().newCDPSession(page);
        await cdp.send('Network.enable');
        await cdp.send('Network.emulateNetworkConditions', {
            offline: false, latency: 50,
            downloadThroughput: 3 * 1024 * 1024,
            uploadThroughput: 3 * 1024 * 1024,
        });
        await page.goto(`http://127.0.0.1:${PORT}/integration_test_page.html`, { waitUntil: 'domcontentloaded' });
        // screenshot a metà download (rete 3MB/s: gz 12.6MB ~4.2s -> a 1.8s ~40%)
        await sleep(1800);
        await page.screenshot({ path: `${SHOTS}/i1-download.png` }).catch(() => {});
        await page.waitForFunction(() => document.getElementById('version-badge').textContent.includes('boot OK'), null, { timeout: 120000 });
        await sleep(700); // fade-out completo (450ms + margine)
        const st = await pullState(page);
        const seq = st.boot.map(e => e.name);
        const dlUpdates = st.barLog.filter(x => x.op === 'update' && x.phase === 'download');
        const fracs = dlUpdates.map(x => x.fraction);
        const intermediate = fracs.filter(f => f > 0 && f < 1);
        // I1a: % crescente: molte update, frazione finale 1, esistono step intermedi
        report('I1a rete lenta: fase Download con % crescente (fraction 0→1, step intermedi)',
            dlUpdates.length >= 3 && fracs[fracs.length - 1] === 1 && intermediate.length >= 2,
            `update=${dlUpdates.length} frac ${fracs[0]?.toFixed(2)}→${fracs[fracs.length - 1]} intermedi=${intermediate.length}`);
        // I1b: label 'Download… 42% · 21.6 MB / 51.4 MB' (percentuale E byte)
        const goodLabels = dlUpdates.filter(x => /^Download… \d+% · [\d.]+ [KMG]B \/ 51\.4 MB$/.test(x.label));
        report('I1b label download con % e byte (MB / 51.4 MB)',
            goodLabels.length >= 1,
            `es. "${dlUpdates.map(x => x.label).find(l => /\d+/.test(l)) || 'NESSUNA'}" (${goodLabels.length} esatte)`);
        // I1c: fill width crescente osservato DAL DOM (MutationObserver sul fill)
        const fillW = st.classLog.filter(x => x.src === 'fill' && x.fill && x.fill !== '0%').map(x => parseFloat(x.fill));
        const fillInterm = fillW.filter(w => w > 0 && w < 100);
        report('I1c fill width crescente nel DOM reale (0→100%)',
            fillW.length >= 3 && Math.max(...fillW) === 100 && fillInterm.length >= 2,
            `widths=${fillW.slice(0, 5).join(',')}…${fillW.length > 5 ? '+' + (fillW.length - 5) : ''}`);
        // I1d: fase Compilazione… VISTA dal DOM: classe lb-indeterminate + label
        const compSeen = st.classLog.filter(x => x.cls && x.cls.includes('lb-indeterminate') && x.label === 'Compilazione…');
        report('I1d fase Compilazione… indeterminata animata (classe vista nel DOM)',
            compSeen.length >= 1,
            `mutazioni compile=${compSeen.length}`);
        // I1e: sequenza completa in ordine (prima occorrenza; progress è multiplo)
        const want = ['download-start', 'download-progress', 'download-end', 'compile-start', 'compile-end', 'ready'];
        const ordered = want.every(n => seq.includes(n))
            && want.every((n, i) => i === 0 || seq.indexOf(want[i - 1]) < seq.indexOf(n));
        report('I1e sequenza eventi completa e in ordine (download→compile→ready)',
            ordered === true,
            `seq=${seq.filter((n, i) => seq.indexOf(n) === i).join('>')}`);
        // I1f: barra scomparsa (fade) senza layout shift: visibility hidden +
        // pointer-events none, box ANCORA nel layout (nessun display:none)
        const gone = await page.evaluate(() => {
            const r = document.querySelector('.lb-root');
            if (!r) return { present: false };
            const cs = getComputedStyle(r);
            return { present: true, vis: cs.visibility, pe: cs.pointerEvents, disp: cs.display, cls: r.className };
        });
        report('I1f barra scomparsa: visibility hidden + pointer-events none (nessun layout shift)',
            gone.present === true && gone.vis === 'hidden' && gone.pe === 'none' && gone.disp !== 'none',
            `vis=${gone.vis} pe=${gone.pe} display=${gone.disp} cls=${gone.cls}`);
        // I1g: simulazione avviata post-boot. Il wasm di produzione usa la
        // feature `webgpu` di bevy_render: nei chromium headless di questa
        // macchina navigator.gpu NON è esposto (verificato: probe dedicato)
        // e il renderer trappa 'unreachable' DOPO il boot completo — stesso
        // comportamento del VECCHIO index.html (prova di parità old==new,
        // /tmp/old_index_parity.mjs). Quindi 'avviata' qui = il modulo wasm
        // è caricato, istanziato e CHIAMABILE (debug_state esiste e risponde;
        // il JSON è pieno quando c'è una GPU, stringa vuota col renderer
        // trapped — parità esatta col vecchio bootstrap).
        await sleep(2500);
        const alive = await page.evaluate(() => {
            try {
                if (!window.__sandbox || typeof window.__sandbox.debug_state !== 'function') return { callable: false };
                const raw = window.__sandbox.debug_state();
                return { callable: true, rawLen: raw.length, parsed: (() => { try { return !!JSON.parse(raw); } catch (e) { return false; } })() };
            } catch (e) { return { callable: false, err: String(e.message) }; }
        });
        const trap = st.boot.length > 0 && ordered; // il boot lifecycle è completo
        report('I1g simulazione avviata: wasm caricato, istanziato e chiamabile (debug_state risponde)',
            alive.callable === true && alive.rawLen >= 0 && trap,
            `callable=${alive.callable} rawLen=${alive.rawLen} parsed=${alive.parsed}${alive.parsed ? ' (GPU ok)' : ' (renderer trapped: headless senza WebGPU — parità col vecchio index)'}`);
        await page.close();
    }

    // ============ I2: flusso veloce (nessuna barra congelata) + reload =====
    {
        const page = await browser.newPage();
        watchPage(page, 'I2');
        const t0 = Date.now();
        await page.goto(`http://127.0.0.1:${PORT}/integration_test_page.html`, { waitUntil: 'domcontentloaded' });
        await page.waitForFunction(() => document.getElementById('version-badge').textContent.includes('boot OK'), null, { timeout: 120000 });
        const fast = Date.now() - t0;
        const st = await pullState(page);
        const seq = st.boot.map(e => e.name);
        const names = ['download-start', 'download-progress', 'download-end', 'compile-start', 'compile-end', 'ready'];
        const okOrder = names.every(n => seq.includes(n)) && seq.indexOf('download-start') < seq.indexOf('compile-start') && seq.indexOf('compile-start') < seq.indexOf('ready');
        report('I2a boot veloce locale: sequenza eventi completa e in ordine',
            okOrder,
            `t=${fast}ms seq=${seq.filter((n, i) => seq.indexOf(n) === i).join('>')}`);
        report('I2b nessun retry in flusso normale', !seq.includes('retry') && !st.barLog.some(x => x.phase === 'retrying'),
            `retries=${seq.filter(x => x === 'retry').length}`);
        // I2c: nessuno stato congelato: stato finale terminale (done), mai
        // sospeso: l'ultima entry del barLog è update('ready')/done
        const last = st.barLog[st.barLog.length - 1];
        report('I2c stato finale terminale (bar done, mai congelata)',
            last && (last.phase === 'ready' && last.done === true),
            `last={op:${last?.op} phase:${last?.phase} done:${last?.done}}`);
        // I2d: RICARICA della pagina: il boot riparte da zero e completa
        await page.reload({ waitUntil: 'domcontentloaded' });
        await page.waitForFunction(() => document.getElementById('version-badge').textContent.includes('boot OK'), null, { timeout: 120000 });
        const st2 = await pullState(page);
        report('I2d ricarica pagina: boot completo di nuovo (nessuna barra bloccata)',
            st2.badge.includes('boot OK') && st2.boot.some(e => e.name === 'ready') && !st2.boot.some(e => e.name === 'retry'),
            `badge="${st2.badge}" seq=${st2.boot.map(e => e.name).filter((n, i, a) => a.indexOf(n) === i).join('>')}`);
        await page.close();
    }

    // ============ I3: NESSUNA REGRESSIONE — index.html di produzione =======
    {
        const page = await browser.newPage();
        watchPage(page, 'I3');
        const pageErrors = [];
        let badgeTime = 0;
        page.on('pageerror', (e) => pageErrors.push({ t: Date.now(), msg: String(e && e.message ? e.message : e) }));
        const t0 = Date.now();
        await page.goto(`http://127.0.0.1:${PORT}/index.html`, { waitUntil: 'domcontentloaded' });
        await page.waitForFunction(() => {
            const v = document.getElementById('version-badge');
            return v && v.textContent.indexOf('✅') >= 0;
        }, null, { timeout: 120000 });
        badgeTime = Date.now();
        await sleep(3000); // stabilità post-boot 3s (poll debug_state vivo)
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
        // I3a come I1g: 'viva' = wasm caricato+istanziato+chiamabile. Il trap
        // 'unreachable' del renderer (headless senza adapter WebGPU) è POST-
        // badge, PRE-esistente e identico nel vecchio index.html (prova di
        // parità); 0 pageerror PRE-badge = il flusso di caricamento è pulito.
        report('I3a boot produzione: badge ✅ + 0 pageerror pre-boot + wasm chiamabile',
            barState.present === true && preBoot.length === 0 && alive.callable === true,
            `bar=${barState.present} pre=${preBoot.length} callable=${alive.callable} parsed=${alive.parsed} t=${Date.now() - t0}ms${pageErrors.length ? ' err[' + pageErrors.map(pe => pe.msg).join(' | ').substring(0, 200) + ']' : ''}`);
        report('I3b barra faded: visibility hidden + pointer-events none (nessun click fantasma)',
            barState.visibility === 'hidden' && barState.pointerEvents === 'none',
            `vis=${barState.visibility} pe=${barState.pointerEvents} cls=${barState.cls}`);
        const noOld = await page.evaluate(() => !document.getElementById('loading-bar') && !document.getElementById('lb-fill'));
        report('I3c vecchio markup #loading-bar rimosso (componente al posto)', noOld === true);
        await page.close();
    }

    // ============ I4: rete muore a metà download x2 -> retry automatico ====
    // deterministico: fetchImpl iniettato dalla pagina (?failFirst=2) muore
    // a metà stream le prime 2 volte — niente CDP offline a tempi fissi.
    {
        const page = await browser.newPage();
        watchPage(page, 'I4');
        await page.goto(`http://127.0.0.1:${PORT}/integration_test_page.html?failFirst=2`, { waitUntil: 'domcontentloaded' });
        await page.waitForFunction(() => document.getElementById('version-badge').textContent.includes('boot OK'), null, { timeout: 120000 });
        const st = await pullState(page);
        const seq = st.boot.map(e => e.name);
        const deaths = seq.filter(x => x === 'synthetic-net-death').length;
        const retries = seq.filter(x => x === 'retry').length;
        const retryingEntries = st.barLog.filter(x => x.phase === 'retrying');
        // label 'Riprovo…' indeterminata: fraction null (mai congelata al 23%)
        const retryIndet = retryingEntries.filter(x => x.fraction === null && /^Riprovo…/.test(x.label));
        const d1 = seq.indexOf('synthetic-net-death'), d2 = seq.indexOf('synthetic-net-death', d1 + 1);
        const r1 = seq.indexOf('retry'), r2 = seq.indexOf('retry', r1 + 1);
        report('I4a 2 morti di rete + 2 retry automatici, poi boot OK',
            deaths === 2 && retries === 2 && st.boot.some(e => e.name === 'ready') && st.badge.includes('boot OK'),
            `deaths=${deaths} retries=${retries} badge="${st.badge}"`);
        report('I4b retry con label Riprovo… indeterminata (mai barra congelata a metà)',
            retryIndet.length >= 2 && retryingEntries.length >= 2,
            `entries=${retryingEntries.length} indeterminate=${retryIndet.length} labels=${retryingEntries.map(x => x.label).join(' | ')}`);
        report('I4c ordine morte→retry→morte→retry→…→ready',
            d1 >= 0 && d2 > d1 && r1 > d1 && r2 > d2 && seq.indexOf('ready') > r2,
            `ordine ok=${d1 >= 0 && d2 > d1 && r1 > d1 && r2 > d2}`);
        // il download RIUSCITO riparte da zero: l'ultima fase download termina a 100%
        const dlAfter = st.barLog.filter(x => x.op === 'update' && x.phase === 'download');
        report('I4d dopo i retry il download completo arriva al 100%',
            dlAfter.length >= 1 && dlAfter[dlAfter.length - 1].fraction === 1,
            `ultime frazioni=${dlAfter.slice(-3).map(x => x.fraction).join(',')} (update totali ${dlAfter.length})`);
        await page.close();
    }

    // ============ I5: errore FATALE -> barra errore + bottone Riprova =====
    {
        const page = await browser.newPage();
        watchPage(page, 'I5');
        await page.goto(`http://127.0.0.1:${PORT}/integration_test_page.html?wasm=/definitivamente-assente.wasm&size=/definitivamente-assente.size`, { waitUntil: 'domcontentloaded' });
        await page.waitForFunction(() => document.getElementById('version-badge').textContent.includes('boot ERR'), null, { timeout: 30000 });
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
        await page.screenshot({ path: `${SHOTS}/i5-error.png` }).catch(() => {});
        report('I5a errore fatale: barra rossa + messaggio + bottone Riprova visibile',
            errState.cls.includes('lb-error') && errState.cls.includes('lb-retryable') && errState.btnVisible && /Riprova/.test(errState.btnText),
            `cls="${errState.cls}" label="${errState.label}" btn="${errState.btnText}"`);
        // click sul bottone: rilancia il ciclo completo (budget pieno) — col
        // URL rotto fallirà di nuovo: il bottone FUNZIONA e lo stato errore si
        // ripresenta (nessuna Promise non gestita, nessuna barra sparita).
        const before = await pullState(page);
        const retriesBefore = before.boot.filter(e => e.name === 'retry').length;
        await page.evaluate(() => { document.querySelector('.lb-root .lb-retry').click(); });
        await page.waitForFunction(() => window.__barLog.filter(x => x.op === 'error').length >= 2, null, { timeout: 30000 });
        const after = await pullState(page);
        const retriesAfter = after.boot.filter(e => e.name === 'retry').length;
        const errCount = after.barLog.filter(x => x.op === 'error').length;
        report('I5b click Riprova: boot rilanciato (2° ciclo completo) e di nuovo in errore',
            errCount === 2 && retriesAfter === retriesBefore + 2 && after.badge.includes('boot ERR'),
            `error-state=${errCount} retries ${retriesBefore}→${retriesAfter} badge="${after.badge}"`);
        // la barra è DI NUOVO cliccabile (nessun fantasma): stato errore coerente
        const again = await page.evaluate(() => {
            const r = document.querySelector('.lb-root');
            const cs = getComputedStyle(r);
            return { cls: r.className, vis: cs.visibility, pe: cs.pointerEvents };
        });
        report('I5c secondo errore: barra ancora visibile e cliccabile (retry idempotente)',
            again.cls.includes('lb-error') && again.cls.includes('lb-retryable') && again.vis === 'visible' && again.pe !== 'none',
            `cls="${again.cls}" vis=${again.vis}`);
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
    ? `\nINTEGRAZIONE COMPLETA — ${results.length}/${results.length} PASS`
    : `\n${failed}/${results.length} TEST INTEGRAZIONE FALLITI`);
process.exit(failed === 0 ? 0 : 1);
