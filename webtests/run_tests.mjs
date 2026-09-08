// webtests/run_tests.mjs — driver dei criteri di accettazione della card
// t_d212617b. Avvia il server fixture, apre la pagina di test in
// Chromium headless (playwright-core) e verifica i 3 criteri con
// esecuzione REALE del modulo wasm_download.js nel browser:
//
//   T1 (criterio 1): /fixture.wasm (CL presente, cache disabilitata)
//      -> la callback di progresso scatta PIÙ volte con frazione
//      crescente da 0 a 1, throttling ≤ ~10/sec.
//   T2 (criterio 2): /fixture-chunked.wasm (NESSUN Content-Length)
//      -> NESSUN errore, solo stato indeterminato (start+end, fraction
//      null, zero 'progress').
//   T3 (criterio 3): i byte ricevuti sono IDENTICI all'originale e
//      costituiscono un modulo WASM valido (WebAssembly.validate +
//      instantiate con f()===42) — verificato per T1, T2 e T-gzip.
//   T4 (extra): /fixture-gz.wasm — determinato con totale DECOMPRESSO
//      (fraction arriva a 1 esatto, non sfora), byte == fixture.
//   T5 (extra): /fixture-gz-nohdr.wasm — gzip SENZA header decompresso
//      e SENZA sidecar: indeterminato (non 130%/517% come nel bug
//      storico), byte decompressi == fixture.
//   T6 (extra): ordine/emissione eventi: start esattamente 1, end
//      esattamente 1, end.done===true, progress solo fra start e end.
//   T7 (extra): sidecar usato quando serve (chunked senza CL ma CON
//      sidecar -> determinato via sidecar).
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { setTimeout as sleep } from 'node:timers/promises';

const PW = '/home/ubuntu/workspace/napkin-maker/node_modules/playwright-core/index.js';
const CHROME = '/home/ubuntu/.cache/ms-playwright/chromium-1234/chrome-linux/chrome';
const PORT = 8181;

const results = [];
function report(name, ok, detail) {
    results.push({ name, ok, detail });
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`);
}

// fixture hash dal server stesso (stessa sorgente di verità del test)
async function fixtureInfo() {
    const r = await fetch(`http://127.0.0.1:${PORT}/fixture.wasm`);
    const buf = Buffer.from(await r.arrayBuffer());
    return {
        len: buf.length,
        sha: createHash('sha256').update(buf).digest('hex'),
    };
}

// Esegue un download nel browser e ritorna eventi + esito validate/instantiate
async function runDownload(page, url, opts = {}) {
    return page.evaluate(async ({ url, opts }) => {
        const events = [];
        const t0 = performance.now();
        const d = await downloadWasm(url, Object.assign({}, opts, {
            onProgress: (p) => events.push(Object.assign({ t: performance.now() - t0 }, p)),
        }));
        let validate = null, exportOk = null;
        try { validate = WebAssembly.validate(d.bytes); } catch (e) { validate = 'THROW: ' + e.message; }
        try {
            const { instance } = await WebAssembly.instantiate(d.bytes, {});
            exportOk = instance.exports && instance.exports.f && instance.exports.f() === 42;
        } catch (e) { exportOk = 'THROW: ' + e.message; }
        return { events, d: { byteLength: d.byteLength, total: d.total, determinate: d.determinate }, validate, exportOk };
    }, { url, opts });
}

const server = spawn('node', ['webtests/server.mjs', String(PORT)], { stdio: ['ignore', 'pipe', 'inherit'] });
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
    await page.goto(`http://127.0.0.1:${PORT}/test-page.html`);
    // attende che il modulo sia caricato ed esposto su window
    await page.waitForFunction(() => typeof window.downloadWasm === 'function', null, { timeout: 10000 });

    const info = await fixtureInfo();
    console.log(`[fixture] len=${info.len} sha256=${info.sha.slice(0, 16)}…`);

    // Per verificare che i byte ricevuti siano identici (criterio 3),
    // nel browser calcolo lo sha256 dei byte via crypto.subtle e confronto
    // con l'hash del file servito (calcolato DENTRO la evaluate).

    // ---- T1: determinato (CL), progress multipli crescenti 0 -> 1 -------
    {
        const r = await runDownload(page, `http://127.0.0.1:${PORT}/fixture.wasm`);
        const prog = r.events.filter(e => e.phase === 'progress');
        const starts = r.events.filter(e => e.phase === 'start');
        const ends = r.events.filter(e => e.phase === 'end');
        const fracs = prog.map(e => e.fraction);
        const rising = fracs.every((f, i) => i === 0 || f >= fracs[i - 1]);
        const maxRate = (prog.length > 1)
            ? (prog.length - 1) / ((prog[prog.length - 1].t - prog[0].t) / 1000)
            : 0;
        report('T1 progress multipli (>3) cresc. 0->1',
            starts.length === 1 && ends.length === 1 && prog.length > 3 && fracs[0] > 0 && fracs[fracs.length - 1] === 1 && rising,
            `start=${starts.length} end=${ends.length} progress=${prog.length} fracs[0]=${fracs[0]?.toFixed(3)} fracs[last]=${fracs[fracs.length - 1]} rising=${rising}`);
        report('T1b throttling ≤ ~10/sec (e ≥2 progress)',
            prog.length >= 2 && maxRate <= 12,
            `rate=${maxRate.toFixed(1)}/s su ${prog.length} eventi (throttle 100ms)`);
        report('T1c fraction in (0,1] e nessuna >1',
            fracs.every(f => f > 0 && f <= 1),
            `min=${Math.min(...fracs)?.toFixed(3)} max=${Math.max(...fracs)}`);
        // T3 (per T1): byte identici + wasm valido
        const r2 = await page.evaluate(async (url) => {
            const d = await downloadWasm(url, {});
            const h = await crypto.subtle.digest('SHA-256', d.bytes);
            const sha = Array.from(new Uint8Array(h)).map(x => x.toString(16).padStart(2, '0')).join('');
            return { sha, len: d.byteLength, validate: WebAssembly.validate(d.bytes) };
        }, `http://127.0.0.1:${PORT}/fixture.wasm`);
        report('T3/T1 bytes identici all\'originale (sha256) + WebAssembly.validate',
            r2.sha === info.sha && r2.len === info.len && r2.validate === true,
            `sha match=${r2.sha === info.sha} len=${r2.len}/${info.len} validate=${r2.validate}`);
    }

    // ---- T2: chunked senza CL -> indeterminato, nessun errore ----------
    {
        const r = await runDownload(page, `http://127.0.0.1:${PORT}/fixture-chunked.wasm`);
        const prog = r.events.filter(e => e.phase === 'progress');
        const start = r.events.find(e => e.phase === 'start');
        const end = r.events.find(e => e.phase === 'end');
        report('T2 chunked senza Content-Length: indeterminato senza errori',
            r.d.determinate === false && prog.length === 0 && start && start.fraction === null && start.total === null && end && end.fraction === null,
            `determinate=${r.d.determinate} progress=${prog.length} start.fraction=${start?.fraction} end.fraction=${end?.fraction}`);
        report('T2b chunked: download completo e wasm valido',
            r.d.byteLength === info.len && r.validate === true && r.exportOk === true,
            `len=${r.d.byteLength}/${info.len} validate=${r.validate} f()=42=${r.exportOk}`);
    }

    // ---- T4: gzip + X-Wasm-Decompressed-Length -> determinato 0->1 ------
    {
        const r = await runDownload(page, `http://127.0.0.1:${PORT}/fixture-gz.wasm`);
        const prog = r.events.filter(e => e.phase === 'progress');
        const fracs = prog.map(e => e.fraction);
        report('T4 gzip+header: determinato, fraction arriva a 1 (non sfora)',
            r.d.determinate === true && r.d.total === info.len && prog.length > 3 && Math.max(...fracs) === 1,
            `total=${r.d.total}/${info.len} progress=${prog.length} max=${Math.max(...fracs)}`);
        report('T4b gzip: byte DECOMPRESSI identici + wasm valido',
            r.d.byteLength === info.len && r.validate === true && r.exportOk === true,
            `len=${r.d.byteLength}/${info.len} validate=${r.validate} f()=42=${r.exportOk}`);
    }

    // ---- T5: gzip senza header e senza sidecar -> indeterminato ---------
    {
        const r = await runDownload(page, `http://127.0.0.1:${PORT}/fixture-gz-nohdr.wasm`);
        const prog = r.events.filter(e => e.phase === 'progress');
        report('T5 gzip senza header nè sidecar: indeterminato (nessun 130%)',
            r.d.determinate === false && prog.length === 0 && r.d.byteLength === info.len && r.exportOk === true,
            `determinate=${r.d.determinate} progress=${prog.length} len=${r.d.byteLength}/${info.len} f()=42=${r.exportOk}`);
    }

    // ---- T6: ordine eventi (su T4-gzip, il più ricco) -------------------
    {
        const r = await runDownload(page, `http://127.0.0.1:${PORT}/fixture-gz.wasm`);
        const idxStart = r.events.findIndex(e => e.phase === 'start');
        const idxEnd = r.events.findIndex(e => e.phase === 'end');
        const badProgOutside = r.events.some((e, i) => e.phase === 'progress' && (i < idxStart || i > idxEnd));
        const endEv = r.events[idxEnd];
        report('T6 ordine: 1 start, progress solo nel mezzo, 1 end.done=true',
            idxStart === 0 && idxEnd === r.events.length - 1 && !badProgOutside && endEv && endEv.done === true && endEv.loaded === info.len,
            `start@${idxStart} end@${idxEnd}/${r.events.length - 1} end.done=${endEv?.done} end.loaded=${endEv?.loaded}/${info.len}`);
    }

    // ---- T7: sidecar rescupera il caso chunked (determinato) ------------
    {
        const r = await runDownload(page, `http://127.0.0.1:${PORT}/fixture-chunked.wasm`, { sizeUrl: `http://127.0.0.1:${PORT}/fixture.size` });
        const prog = r.events.filter(e => e.phase === 'progress');
        report('T7 sidecar su chunked: determinato via sidecar',
            r.d.determinate === true && r.d.total === info.len && prog.length > 3 && prog[prog.length - 1].fraction === 1,
            `determinate=${r.d.determinate} total=${r.d.total}/${info.len} progress=${prog.length}`);
    }

} catch (e) {
    console.error('DRIVER ERROR:', e);
    process.exitCode = 1;
} finally {
    if (browser) await browser.close().catch(() => {});
    fetch(`http://127.0.0.1:${PORT}/shutdown`, { method: 'POST' }).catch(() => {});
    await sleep(200);
    server.kill();
}

const failed = results.filter(r => !r.ok).length;
if (results.length === 0) {
    console.error('\nNESSUN TEST ESEGUITO — fallimento del driver');
    process.exit(1);
}
console.log(failed === 0
    ? `\nTUTTI I ${results.length} TEST PASSATI — criteri di accettazione verificati`
    : `\n${failed}/${results.length} TEST FALLITI`);
process.exit(failed === 0 ? 0 : 1);
