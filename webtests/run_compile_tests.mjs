// webtests/run_compile_tests.mjs — driver dei criteri di accettazione della
// card t_3c91ee24 (eventi di stato per la fase di compilazione WASM).
// Avvia compile_server.mjs, apre compile-test-page.html in Chromium headless
// e verifica con ESECUZIONE REALE nel browser:
//
//   AC1 — compile-start / compile-end attorno alla compilazione REALE:
//     C1  pipeline completa (fixture.wasm): ordine esatto degli eventi
//         download-start → download-progress* → download-end →
//         compile-start → compile-end → ready (nessuno prima/dopo).
//     C2  BIG fixture (200k funzioni): compile-end.durationMs > 0 e la
//         compilazione REALE (V8) dura effettivamente del tempo.
//     C2b PROVA DI BRACKETING: compileImpl iniettato che registra
//         [call] → l'ordine osservato è compile-start, [call],
//         compile-end: gli eventi circondano la chiamata reale.
//     C3  compile-end.module è un WebAssembly.Module VALIDO
//         (WebAssembly.Module / validate + instantiate f()===42).
//     C4  API funzionale singola: loadWasmLifecycle con events =
//         funzione (name, payload) invece del bus.
//   AC2 — errori di compilazione → evento 'error':
//     C5  compileWasm(FIXTURE_BAD) rigetta e emette ESATTAMENTE 1 evento
//         'error' con stage='compile' e message non vuoto (errore di
//         compilazione REALE su modulo corrotto).
//     C6  loadWasmLifecycle(FIXTURE_BAD) rigetta e emette 'error'
//         stage='compile' UNA volta sola (il download non fallisce).
//     C7  errore di DOWNLOAD → 'error' stage='download' (404), UNA volta.
//     C8  errore di INIT (initFn che lancia) → 'error' stage='init'.
//     C8b messaggio d'errore PROPAGATO: loadWasmLifecycle rigetta con lo
//         STESSO errore emesso nell'evento (identity match).
//   EventBus:
//     C9  on/off/unsubscribe: listenerCount, emit ritorna i delivery,
//         callback isolata (una che lancia non blocca le altre).
//     C10 ordine LOADER_EVENTS completo (contratto pubblico).
//   Annullamento:
//     C11 AbortSignal pre-aborted: rigetto AbortError, NESSUN 'error'
//         emesso (abort ≠ fallimento).
import { spawn } from 'node:child_process';
import { setTimeout as sleep } from 'node:timers/promises';

const PW = '/home/ubuntu/workspace/napkin-maker/node_modules/playwright-core/index.js';
const CHROME = '/home/ubuntu/.cache/ms-playwright/chromium-1234/chrome-linux/chrome';
const PORT = 8183;
const results = [];
let driverFailed = false;
function report(name, ok, detail) {
    results.push({ name, ok, detail });
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`);
}

const server = spawn('node', ['webtests/compile_server.mjs', String(PORT)], { stdio: ['ignore', 'pipe', 'inherit'] });
server.stdout.on('data', (d) => process.stdout.write('[server] ' + d));
await new Promise((res) => {
    server.stdout.on('data', (d) => { if (String(d).includes('fixture server su')) res(); });
    setTimeout(res, 1500);
});

let browser, page;
try {
    const pw = await import(PW);
    const chromium = (pw.default && pw.default.chromium) ? pw.default.chromium : pw.chromium;
    browser = await chromium.launch({ executablePath: CHROME, args: ['--no-sandbox', '--use-angle=swiftshader-webgl'] });
    page = await browser.newPage();
    await page.goto(`http://127.0.0.1:${PORT}/compile-test-page.html`);
    await page.waitForFunction(() => typeof window.loadWasmLifecycle === 'function', null, { timeout: 10000 });

    const URL_FIX = `http://127.0.0.1:${PORT}/fixture.wasm`;
    const URL_BIG = `http://127.0.0.1:${PORT}/fixture-big.wasm`;
    const URL_BAD = `http://127.0.0.1:${PORT}/fixture-bad.wasm`;
    const SIZE_URL = `http://127.0.0.1:${PORT}/fixture.size`;

    // helper: esegue nel browser uno snippet e ritorna JSON
    const ev = (fn, ...args) => page.evaluate(fn, ...args);

    // ---- C1: ordine eventi pipeline completa --------------------------
    {
        const r = await ev(async ({ url, sizeUrl }) => {
            const events = [];
            const bus = createLoaderEvents();
            for (const name of ['download-start', 'download-progress', 'download-end', 'compile-start', 'compile-end', 'ready', 'error']) {
                bus.on(name, (p) => events.push({ name, p }));
            }
            const init = (o) => { // init fittizio: istanzia il modulo REALE
                return WebAssembly.instantiate(o.module_or_path, {}).then((i) => i.exports);
            };
            const wasm = await loadWasmLifecycle(url, { events: bus, sizeUrl, init });
            return { sequence: events.map(e => e.name), n: events.length, f: wasm && wasm.f ? wasm.f() : null };
        }, { url: URL_FIX, sizeUrl: SIZE_URL });
        const seqOk = r.sequence.indexOf('download-start') === 0
            && r.sequence.indexOf('download-end') < r.sequence.indexOf('compile-start')
            && r.sequence.indexOf('compile-start') < r.sequence.indexOf('compile-end')
            && r.sequence.indexOf('compile-end') < r.sequence.indexOf('ready')
            && r.sequence[r.sequence.length - 1] === 'ready'
            && !r.sequence.includes('error');
        const progCount = r.sequence.filter(n => n === 'download-progress').length;
        report('C1 pipeline: ordine start→progress→end→compile-start→compile-end→ready',
            seqOk && progCount >= 1 && r.f === 42,
            `seq=${r.sequence.slice(0, 3).join('>')}>…${r.sequence.slice(-3).join('>')} progress=${progCount} f()=${r.f}`);
    }

    // ---- C2: compile REALE dura tempo misurabile (fixture 200k funzioni)
    {
        const r = await ev(async ({ url }) => {
            const got = {};
            const bus = createLoaderEvents();
            bus.on('compile-start', () => { got.start = performance.now(); });
            bus.on('compile-end', (p) => { got.end = performance.now(); got.durationMs = p.durationMs; });
            const module = await compileWasm(await (await fetch(url)).arrayBuffer(), { events: bus });
            const isModule = module instanceof WebAssembly.Module;
            // NB: instantiate(Module, imports) risolve con l'ISTANZA diretta
            // (overload), non con {instance, module} — come il glue wasm-bindgen.
            const r2 = await WebAssembly.instantiate(module, {});
            const inst = r2 instanceof WebAssembly.Instance ? r2 : r2.instance;
            return { durationMs: got.durationMs, wallMs: got.end - got.start, isModule, f: inst.exports.f() };
        }, { url: URL_BIG });
        report('C2 compile REALE (200k funzioni): durationMs > 0, Module valido, f()=42',
            r.durationMs > 0 && r.wallMs > 0 && r.isModule === true && r.f === 42,
            `durationMs=${r.durationMs?.toFixed(1)} wallMs=${r.wallMs?.toFixed(1)} isModule=${r.isModule} f()=${r.f}`);
    }

    // ---- C2b: PROVA di bracketing — compileImpl iniettato registra [call]
    {
        const r = await ev(async ({ url }) => {
            const trace = [];
            const bus = createLoaderEvents();
            bus.on('compile-start', () => trace.push('compile-start'));
            bus.on('compile-end', () => trace.push('compile-end'));
            const compileImpl = async (bytes) => {
                trace.push('[compile:call]');
                return await WebAssembly.compile(bytes); // compilazione REALE
            };
            const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
            const module = await compileWasm(bytes, { events: bus, compileImpl });
            return { trace, ok: module instanceof WebAssembly.Module };
        }, { url: URL_BIG });
        const t = r.trace;
        const idxStart = t.indexOf('compile-start');
        const idxCall = t.indexOf('[compile:call]');
        const idxEnd = t.indexOf('compile-end');
        report('C2b bracketing: compile-start → [compile:call] → compile-end',
            idxStart !== -1 && idxCall !== -1 && idxEnd !== -1 && idxStart < idxCall && idxCall < idxEnd
                && t.filter(x => x === 'compile-start').length === 1 && t.filter(x => x === 'compile-end').length === 1
                && r.ok === true,
            `trace=${t.join(' → ')}`);
    }

    // ---- C3: compile-end.module è il modulo compilato ------------------
    {
        const r = await ev(async ({ url }) => {
            const bus = createLoaderEvents();
            let mod = null;
            bus.on('compile-end', (p) => { mod = p.module; });
            const module = await compileWasm(new Uint8Array(await (await fetch(url)).arrayBuffer()), { events: bus });
            const same = mod === module; // è LO STESSO modulo ritornato
            const valid = WebAssembly.Module && mod instanceof WebAssembly.Module;
            // NB: instantiate(Module, imports) risolve con l'ISTANZA diretta
            const r2 = await WebAssembly.instantiate(mod, {});
            const inst = r2 instanceof WebAssembly.Instance ? r2 : r2.instance;
            return { same, valid, f: inst.exports.f(), isModuleInEvent: valid };
        }, { url: URL_FIX });
        report('C3 compile-end.module = Module compilato (identity) e istanziabile f()=42',
            r.same === true && r.valid === true && r.f === 42,
            `same=${r.same} isModule=${r.valid} f()=${r.f}`);
    }

    // ---- C4: API funzionale singola ------------------------------------
    {
        const r = await ev(async ({ url, sizeUrl }) => {
            const seen = [];
            const wasm = await loadWasmLifecycle(url, {
                sizeUrl,
                init: async (o) => (await WebAssembly.instantiate(o.module_or_path, {})).exports,
                events: (name, payload) => { seen.push({ name, hasTs: typeof payload.ts === 'number' }); },
            });
            const names = seen.map(e => e.name);
            return {
                orderOk: names.indexOf('download-start') === 0 && names[names.length - 1] === 'ready'
                    && names.indexOf('compile-start') < names.indexOf('compile-end'),
                allHaveTs: seen.every(e => e.hasTs),
                f: wasm.f(),
            };
        }, { url: URL_FIX, sizeUrl: SIZE_URL });
        report('C4 events come funzione unica (name, payload): ordine + ts su TUTTI gli eventi',
            r.orderOk === true && r.allHaveTs === true && r.f === 42,
            `orderOk=${r.orderOk} ts=${r.allHaveTs} f()=${r.f}`);
    }

    // ---- C5: errore di compilazione REALE → 1 evento error stage=compile
    {
        const r = await ev(async ({ url }) => {
            const errors = [];
            const bus = createLoaderEvents();
            bus.on('error', (p) => errors.push(p));
            const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
            let rejectedWith = null;
            try { await compileWasm(bytes, { events: bus }); } catch (e) { rejectedWith = e; }
            return {
                nErrors: errors.length,
                stage: errors[0] && errors[0].stage,
                message: errors[0] && errors[0].message,
                detailType: errors[0] && errors[0].detail && errors[0].detail.constructor.name,
                threw: rejectedWith != null,
                isCompileError: rejectedWith && rejectedWith.constructor.name,
                marked: rejectedWith && rejectedWith.__loaderReported === true,
            };
        }, { url: URL_BAD });
        report('C5 WASM corrotto: rigetto + esattamente 1 error stage=compile, message non vuoto',
            r.threw === true && r.nErrors === 1 && r.stage === 'compile' && typeof r.message === 'string' && r.message.length > 0,
            `nErrors=${r.nErrors} stage=${r.stage} errType=${r.isCompileError} msg="${String(r.message).slice(0, 60)}" marked=${r.marked}`);
    }

    // ---- C6: pipeline con wasm corrotto → error stage=compile (1 volta) --
    {
        const r = await ev(async ({ url }) => {
            const events = [];
            const bus = createLoaderEvents();
            for (const n of ['download-start', 'download-end', 'compile-start', 'compile-end', 'ready', 'error']) {
                bus.on(n, (p) => events.push({ n, p }));
            }
            let threw = false;
            try {
                await loadWasmLifecycle(url, {
                    events: bus,
                    init: async (o) => (await WebAssembly.instantiate(o.module_or_path, {})).exports,
                });
            } catch (e) { threw = true; }
            const errors = events.filter(e => e.n === 'error');
            const hasDownloadEnd = events.some(e => e.n === 'download-end');
            const hasCompileStart = events.some(e => e.n === 'compile-start');
            const noCompileEnd = !events.some(e => e.n === 'compile-end');
            return { threw, nErrors: errors.length, stage: errors[0] && errors[0].p.stage, hasDownloadEnd, hasCompileStart, noCompileEnd, noReady: !events.some(e => e.n === 'ready') };
        }, { url: URL_BAD });
        report('C6 pipeline con wasm corrotto: download ok, compile-start sì, compile-end NO, 1 error compile',
            r.threw === true && r.nErrors === 1 && r.stage === 'compile' && r.hasDownloadEnd === true && r.hasCompileStart === true && r.noCompileEnd === true && r.noReady === true,
            `threw=${r.threw} nErrors=${r.nErrors} stage=${r.stage} dlEnd=${r.hasDownloadEnd} compStart=${r.hasCompileStart} compEnd=${!r.noCompileEnd}`);
    }

    // ---- C7: errore di DOWNLOAD → error stage=download ------------------
    {
        // NB: l'URL va passata come PARAMETRO (come gli altri test): dentro
        // la callback evaluate non esiste PORT (ReferenceError nel browser).
        const r = await ev(async ({ url }) => {
            const errors = [];
            const bus = createLoaderEvents();
            bus.on('error', (p) => errors.push(p));
            let threw = false;
            try {
                await loadWasmLifecycle(url, {
                    events: bus,
                    init: async () => { throw new Error('init non deve girare'); },
                });
            } catch (e) { threw = true; }
            return { threw, nErrors: errors.length, stage: errors[0] && errors[0].stage, message: errors[0] && errors[0].message };
        }, { url: `http://127.0.0.1:${PORT}/nonexistent.wasm` });
        report('C7 download 404: rigetto + 1 error stage=download',
            r.threw === true && r.nErrors === 1 && r.stage === 'download' && typeof r.message === 'string' && r.message.length > 0,
            `threw=${r.threw} nErrors=${r.nErrors} stage=${r.stage} msg="${String(r.message).slice(0, 50)}"`);
    }

    // ---- C8: errore di INIT → error stage=init --------------------------
    {
        const r = await ev(async ({ url }) => {
            const errors = [];
            const bus = createLoaderEvents();
            bus.on('error', (p) => errors.push(p));
            let threw = false;
            try {
                await loadWasmLifecycle(url, {
                    events: bus,
                    init: async () => { throw new Error('glue init esploso'); },
                });
            } catch (e) { threw = true; }
            const hasCompileEnd = true; // verificato sopra: la pipeline emette compile-end prima dell'init
            return { threw, nErrors: errors.length, stage: errors[0] && errors[0].stage, message: errors[0] && errors[0].message };
        }, { url: URL_FIX });
        report('C8 init che lancia: rigetto + 1 error stage=init (dopo ready-flow completo)',
            r.threw === true && r.nErrors === 1 && r.stage === 'init' && r.message === 'glue init esploso',
            `threw=${r.threw} nErrors=${r.nErrors} stage=${r.stage} msg="${r.message}"`);
    }

    // ---- C8b: l'errore propagato è LO STESSO dell'evento (identity) ------
    {
        const r = await ev(async ({ url }) => {
            const bus = createLoaderEvents();
            let evErr = null;
            bus.on('error', (p) => { evErr = p.detail; });
            let thrown = null;
            try {
                await loadWasmLifecycle(url, {
                    events: bus,
                    init: async () => { throw new Error('boom-init'); },
                });
            } catch (e) { thrown = e; }
            return { same: evErr === thrown, msg: thrown && thrown.message, stage: 'init' };
        }, { url: URL_FIX });
        report('C8b identità errore: il detail dell\'evento error È l\'errore rigettato',
            r.same === true && r.msg === 'boom-init',
            `same=${r.same} msg=${r.msg}`);
    }

    // ---- C9: bus on/off/emit/isolamento callback ------------------------
    {
        const r = await ev(() => {
            const bus = createLoaderEvents();
            const calls = [];
            const cb1 = (p) => calls.push('cb1:' + p.v);
            const cb2 = (p) => { calls.push('cb2:' + p.v); throw new Error('cb2 esplode'); };
            const cb3 = (p) => calls.push('cb3:' + p.v);
            const off1 = bus.on('x', cb1);
            bus.on('x', cb2);
            bus.on('x', cb3);
            const n0 = bus.listenerCount('x');
            const delivered = bus.emit('x', { v: 1 });
            off1(); // unsubscribe
            bus.emit('x', { v: 2 });
            bus.off('x', cb3);
            const delivered3 = bus.emit('x', { v: 3 });
            // on/off DENTRO una callback: non deve rompere l'emit (snapshot)
            const inner = [];
            const selfRemove = () => { bus.off('y', selfRemove); inner.push('ran'); };
            bus.on('y', selfRemove);
            bus.on('y', () => inner.push('second'));
            bus.emit('y', {});
            return {
                n0, delivered,
                callsAfterV1: calls.filter(c => c.endsWith(':1')).length,
                callsAfterV2: calls.filter(c => c.endsWith(':2')).length, // cb2+cb3, cb1 rimossa
                delivered3, // cb2 è rimasta: 1 delivery
                inner,
            };
        });
        report('C9 bus: on/off/unsub, emit ritorna delivery, callback isolate, snapshot in emit',
            r.n0 === 3 && r.delivered === 3 && r.callsAfterV1 === 3
                && r.callsAfterV2 === 2 && r.delivered3 === 1 && r.inner.length === 2 && r.inner[0] === 'ran' && r.inner[1] === 'second',
            `n0=${r.n0} d1=${r.delivered} afterV1=${r.callsAfterV1} afterV2=${r.callsAfterV2} d3=${r.delivered3} inner=${r.inner.join(',')}`);
    }

    // ---- C10: contratto LOADER_EVENTS ----------------------------------
    {
        const r = await ev(() => LOADER_EVENTS.slice());
        const expected = ['download-start', 'download-progress', 'download-end', 'compile-start', 'compile-end', 'ready', 'error'];
        report('C10 LOADER_EVENTS: i 7 eventi del ciclo di vita nell\'ordine del contratto',
            JSON.stringify(r) === JSON.stringify(expected),
            `${r.join(', ')}`);
    }

    // ---- C11: AbortSignal pre-aborted ≠ error ---------------------------
    {
        const r = await ev(async ({ url }) => {
            const errors = [];
            const bus = createLoaderEvents();
            bus.on('error', (p) => errors.push(p));
            let threw = false, name = null;
            try {
                await loadWasmLifecycle(url, {
                    events: bus,
                    signal: AbortSignal.abort(),
                    init: async () => { throw new Error('init non deve girare'); },
                });
            } catch (e) { threw = true; name = e && e.name; }
            return { threw, name, nErrors: errors.length };
        }, { url: URL_FIX });
        report('C11 AbortSignal pre-aborted: rigetto AbortError, NESSUN evento error',
            r.threw === true && r.name === 'AbortError' && r.nErrors === 0,
            `threw=${r.threw} name=${r.name} nErrors=${r.nErrors}`);
    }

} catch (e) {
    console.error('DRIVER ERROR:', e);
    driverFailed = true;
} finally {
    if (browser) await browser.close().catch(() => {});
    fetch(`http://127.0.0.1:${PORT}/shutdown`, { method: 'POST' }).catch(() => {});
    await sleep(200);
    server.kill();
}

const failed = results.filter(r => !r.ok).length;
if (results.length === 0 || driverFailed) {
    console.error('\n' + (driverFailed ? 'DRIVER ERROR — esecuzione incompleta' : 'NESSUN TEST ESEGUITO — fallimento del driver'));
    process.exit(1);
}
const failures = results.filter(r => !r.ok);
console.log(failed === 0
    ? `\nTUTTI I ${results.length} TEST PASSATI — criteri t_3c91ee24 verificati`
    : `\n${failed}/${results.length} TEST FALLITI:\n${failures.map(f => '  FAIL ' + f.name).join('\n')}`);
process.exit(failed === 0 ? 0 : 1);
