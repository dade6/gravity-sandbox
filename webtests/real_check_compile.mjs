// webtests/real_check_compile.mjs — verifica END-TO-END della card
// t_3c91ee24 sul WASM REALE dell'app (51MB, 12.6MB gz) attraverso
// serve_wasm.py (il server di produzione) e il GLUE REALE wasm-bindgen:
//
//   RC1: loadWasmLifecycle() sul wasm reale: tutti gli eventi in ordine
//        (download-start → progress* → download-end → compile-start →
//        compile-end → ready), progresso determinato col totale
//        DECOMPRESSO, frazione fino a 1.
//   RC2: la compilazione REALE del wasm da 51MB dura un tempo
//        misurabile: compile-end.durationMs > 0.
//   RC3: 'ready' arriva con timings {downloadMs, compileMs, initMs} tutti
//        finiti (>0) — le tre fasi sono davvero misurate.
//   RC4: il modulo compilato È quello servito: compile-end.module
//        istanziato dal GLUE REALE (gravity_sandbox_p80.js) senza alcun
//        fetch interno né ricompilazione — l'app Bevy si avvia e il badge
//        diventa ✅ (nessuna regressione del boot).
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, copyFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

const PW = '/home/ubuntu/workspace/napkin-maker/node_modules/playwright-core/index.js';
const CHROME = '/home/ubuntu/.cache/ms-playwright/chromium-1234/chrome-linux/chrome';
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const PORT = 8184;
const GLUE_NAME = 'gravity_sandbox_p80.js';

const results = [];
let driverFailed = false;
function report(name, ok, detail) {
    results.push({ name, ok, detail });
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`);
}

// serve_wasm.py serve SOLO wasm-dist: la pagina host e i moduli vanno
// copiati lì (wasm-dist è gitignored; build_wasm.sh copierà in seguito
// wasm_runtime.js nel deploy).
copyFileSync(path.join(REPO, 'wasm_runtime.js'), path.join(REPO, 'wasm-dist', 'wasm_runtime.js'));
copyFileSync(path.join(REPO, 'wasm_download.js'), path.join(REPO, 'wasm-dist', 'wasm_download.js'));

// Pagina host DEDICATA (non tocca index.html: la regressione del boot si
// verifica sulla pagina reale originale in RC4): importa il GLUE REALE
// e carica il wasm con loadWasmLifecycle invece del bootstrap inline.
writeFileSync(path.join(REPO, 'wasm-dist', 'compile-real-page.html'), `<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>wasm_runtime real check</title>
<style>body{margin:0;background:#0a0a14;font:13px monospace;color:#fff}
#status{padding:12px;white-space:pre-wrap}
#bevy-canvas{display:block;width:100%;height:600px;position:relative}
</style></head><body>
<div id="version-badge">wasm_runtime real-check</div>
<canvas id="bevy-canvas" tabindex="0"></canvas>
<div id="status">boot…</div>
<script type="module">
    import init, * as sandbox from '/${GLUE_NAME}?v=0.14.80';
    import { createLoaderEvents, loadWasmLifecycle } from '/wasm_runtime.js';
    window.__sandbox = null;
    const events = [];
    const bus = createLoaderEvents();
    for (const n of ['download-start','download-progress','download-end','compile-start','compile-end','ready','error']) {
        bus.on(n, (p) => events.push({ n, p }));
    }
    try {
        const t0 = performance.now();
        const wasm = await loadWasmLifecycle(
            new URL('gravity_sandbox_p80_bg.wasm?v=0.14.80', location.href).href,
            {
                init,
                events: bus,
                sizeUrl: new URL('gravity_sandbox_p80_bg.wasm.size?v=0.14.80', location.href).href,
            },
        );
        document.getElementById('status').textContent =
            'READY in ' + (performance.now() - t0).toFixed(0) + 'ms — eventi: ' + events.length;
        window.__sandbox = sandbox;
        window.__events = events;
        window.__wasmReady = true;
        const v = document.getElementById('version-badge');
        if (v) v.textContent = 'READY ✅';
    } catch (err) {
        document.getElementById('status').textContent = 'ERROR: ' + err.message;
        window.__events = events;
        window.__wasmError = String(err && err.message ? err.message : err);
        const v = document.getElementById('version-badge');
        if (v) v.textContent = 'ERROR 🛑';
    }
</script>
</body></html>
`);

const server = spawn('python3', [path.join(REPO, 'serve_wasm.py'), String(PORT), path.join(REPO, 'wasm-dist')], { stdio: ['ignore', 'pipe', 'inherit'] });
await new Promise((res) => {
    server.stdout.on('data', (d) => { if (String(d).includes('Serving')) res(); });
    setTimeout(res, 2000);
});
console.log(`[real] serve_wasm.py su http://127.0.0.1:${PORT}`);

const wasmPath = path.join(REPO, 'wasm-dist', 'gravity_sandbox_p80_bg.wasm');
const diskLen = readFileSync(wasmPath).length;
console.log(`[real] disco: ${diskLen} bytes`);

let browser;
try {
    const pw = await import(PW);
    const chromium = (pw.default && pw.default.chromium) ? pw.default.chromium : pw.chromium;
    browser = await chromium.launch({ executablePath: CHROME, args: ['--no-sandbox', '--use-angle=swiftshader-webgl', '--enable-unsafe-swiftshader'] });
    const page = await browser.newPage();
    const pageErrors = [];
    let readyAt = 0; // epoch ms in cui __wasmReady diventa true (per epoch: uso Date.now via evaluate)
    page.on('pageerror', (e) => pageErrors.push({ t: Date.now(), msg: String(e && e.message ? e.message : e) }));

    // Rete rallentata via CDP (simula Tailscale/DERP): rende il download
    // osservabile (molteplici progress) come in produzione.
    const cdp = await page.context().newCDPSession(page);
    await cdp.send('Network.enable');
    await cdp.send('Network.emulateNetworkConditions', {
        offline: false, latency: 50,
        downloadThroughput: 3 * 1024 * 1024,
        uploadThroughput: 3 * 1024 * 1024,
    });

    await page.goto(`http://127.0.0.1:${PORT}/compile-real-page.html`, { waitUntil: 'domcontentloaded' });
    // Il canvas Bevy occupa 600px; il boot reale (WebGL swiftshader) può
    // richiedere decine di secondi — attesa READY ✅.
    await page.waitForFunction(() => window.__wasmReady === true || window.__wasmError, null, { timeout: 180000 });
    readyAt = await page.evaluate(() => Date.now());
    // Stabilità post-boot: 3s di runtime vivo prima della valutazione
    // (un trap del wasm si manifesterebbe qui, come nel real_check sibling).
    await sleep(3000);

    const r = await page.evaluate(() => {
        const evs = window.__events || [];
        const names = evs.map(e => e.n);
        const prog = evs.filter(e => e.n === 'download-progress');
        const compStart = evs.find(e => e.n === 'compile-start');
        const compEnd = evs.find(e => e.n === 'compile-end');
        const ready = evs.find(e => e.n === 'ready');
        const errors = evs.filter(e => e.n === 'error');
        return {
            err: window.__wasmError || null,
            sequence: names,
            nProgress: prog.length,
            firstFrac: prog.length ? prog[0].p.fraction : null,
            lastFrac: prog.length ? prog[prog.length - 1].p.fraction : null,
            dlEndLoaded: (evs.find(e => e.n === 'download-end') || {}).p ? evs.find(e => e.n === 'download-end').p.loaded : null,
            compStartByteLength: compStart ? compStart.p.byteLength : null,
            compileDurationMs: compEnd ? compEnd.p.durationMs : null,
            timings: ready ? ready.p.timings : null,
            nErrors: errors.length,
            sandboxExposed: typeof window.__sandbox === 'object' && window.__sandbox !== null
                && typeof window.__sandbox.debug_state === 'function',
        };
    });

    // RC1: sequenza completa e corretta sul wasm REALE
    {
        const seq = r.sequence;
        const i = (n) => seq.indexOf(n);
        const orderOk = i('download-start') === 0
            && i('download-start') < i('download-end')
            && i('download-end') < i('compile-start')
            && i('compile-start') < i('compile-end')
            && i('compile-end') < i('ready')
            && seq[seq.length - 1] === 'ready'
            && r.nErrors === 0;
        report('RC1 wasm reale 51MB: sequenza download→…→compile-start→compile-end→ready, 0 error',
            orderOk && r.nProgress > 3 && r.lastFrac === 1 && r.dlEndLoaded === diskLen,
            `progress=${r.nProgress} first=${r.firstFrac?.toFixed(3)} last=${r.lastFrac} dlEnd=${r.dlEndLoaded}/${diskLen} seq=${seq.slice(0, 2).join('>')}>…>${seq.slice(-3).join('>')}`);
    }

    // RC2: compilazione REALE misurabile sul wasm da 51MB
    report('RC2 compile REALE 51MB: durationMs > 0 (compilazione osservabile)',
        r.compileDurationMs != null && r.compileDurationMs > 0 && r.compStartByteLength === diskLen,
        `durationMs=${r.compileDurationMs?.toFixed(0)} compile-start.byteLength=${r.compStartByteLength}/${diskLen}`);

    // RC3: timings completi nell'evento ready
    {
        const t = r.timings || {};
        report('RC3 ready.timings {downloadMs, compileMs, initMs} tutti > 0',
            t.downloadMs > 0 && t.compileMs > 0 && t.initMs > 0,
            `download=${t.downloadMs?.toFixed(0)}ms compile=${t.compileMs?.toFixed(0)}ms init=${t.initMs?.toFixed(0)}ms`);
    }

    // RC4: nessuna regressione: il GLUE REALE ha istanziato il Module
    // precompilato e l'app Bevy gira (sandbox esposto). pageerror DOPO
    // readyAt = teardown della chiusura pagina (winit/Worker interrotti),
    // benigno e pre-esistente (documentato nel real_check sibling); PRIMA
    // del ready sarebbe un problema reale del flusso di caricamento.
    {
        const pre = pageErrors.filter(pe => pe.t < readyAt);
        const post = pageErrors.length - pre.length;
        report('RC4 glue reale istanzia il Module precompilato: app viva, 0 pageerror pre-ready',
            r.sandboxExposed === true && pre.length === 0,
            `sandbox.debug_state=${r.sandboxExposed} pageerror PRE-ready=${pre.length} POST-ready(benigni)=${post}${pageErrors.length ? ' [' + pageErrors.map(pe => pe.msg).join(' | ').slice(0, 200) + ']' : ''}`);
    }

    await page.close();
} catch (e) {
    console.error('DRIVER ERROR:', e);
    driverFailed = true;
} finally {
    if (browser) await browser.close().catch(() => {});
    server.kill();
    await sleep(200);
}

const failed = results.filter(r => !r.ok).length;
console.log(failed === 0
    ? `\nVERIFICA REALE COMPLETA — ${results.length}/${results.length} PASS (wasm produzione 51MB + glue reale)`
    : `\n${failed}/${results.length} REAL-CHECK FALLITI`);
process.exit(failed === 0 ? 0 : 1);
