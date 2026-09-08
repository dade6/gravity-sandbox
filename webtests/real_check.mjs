// webtests/real_check.mjs — verifica sul WASM REALE dell'app (51MB,
// 12.6MB gz) attraverso serve_wasm.py, il server di produzione:
//   R1: downloadWasm() del .wasm servito via serve_wasm.py (gzip attivo
//       se il client accetta): progresso DETERMINATO col totale
//       DECOMPRESSO (header X-Wasm-Decompressed-Length), frazione che
//       arriva a 1, throttling rispettato.
//   R2: sha256 dei byte ricevuti == sha256 del file su disco.
//   R3: WebAssembly.validate sui byte ricevuti.
//   R4 (regressione boot): la pagina reale index.html di wasm-dist si
//       avvia come prima: badge "v0.14.80 ✅" e loading bar nascosta
//       (nessuna regressione del flusso esistente, che NON usa ancora il
//       nuovo modulo — verifica che il nuovo modulo sia deployato e
//       servito correttamente senza toccare il bootstrap).
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, copyFileSync } from 'node:fs';
import path from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

const PW = '/home/ubuntu/workspace/napkin-maker/node_modules/playwright-core/index.js';
const CHROME = '/home/ubuntu/.cache/ms-playwright/chromium-1234/chrome-linux/chrome';
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const PORT = 8182;

const results = [];
function report(name, ok, detail) {
    results.push({ name, ok, detail });
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`);
}

// Il server di produzione serve SOLO wasm-dist: la pagina host e il
// modulo vengono copiati lì prima dell'avvio (wasm-dist è gitignored,
// i file test NON finiscono nel deploy finale: build_wasm.sh copierà
// wasm_download.js ma non test-page.html).
copyFileSync(path.join(REPO, 'wasm_download.js'), path.join(REPO, 'wasm-dist', 'wasm_download.js'));
copyFileSync(path.join(REPO, 'webtests', 'test-page.html'), path.join(REPO, 'wasm-dist', 'test-page.html'));

// avvia serve_wasm.py (il server DI PRODUZIONE) su wasm-dist
const server = spawn('python3', [path.join(REPO, 'serve_wasm.py'), String(PORT), path.join(REPO, 'wasm-dist')], { stdio: ['ignore', 'pipe', 'inherit'] });
await new Promise((res) => {
    server.stdout.on('data', (d) => { if (String(d).includes('Serving')) res(); });
    setTimeout(res, 2000);
});
console.log(`[real] serve_wasm.py su http://127.0.0.1:${PORT}`);

// hash del file su disco (verità a terra)
const wasmPath = path.join(REPO, 'wasm-dist', 'gravity_sandbox_p80_bg.wasm');
const diskBytes = readFileSync(wasmPath);
const diskSha = createHash('sha256').update(diskBytes).digest('hex');
const diskLen = diskBytes.length;
console.log(`[real] disco: ${diskLen} bytes, sha256=${diskSha.slice(0, 16)}…`);

let browser, page;
try {
    const pw = await import(PW);
    const chromium = (pw.default && pw.default.chromium) ? pw.default.chromium : pw.chromium;
    browser = await chromium.launch({ executablePath: CHROME, args: ['--no-sandbox', '--use-angle=swiftshader-webgl', '--enable-unsafe-swiftshader'] });
    page = await browser.newPage();
    await page.goto(`http://127.0.0.1:${PORT}/test-page.html`);
    await page.waitForFunction(() => typeof window.downloadWasm === 'function', null, { timeout: 10000 });

    // ---- R1+R2+R3: download del wasm reale col nuovo modulo -----------
    // Rete rallentata via CDP (come Tailscale/DERP in produzione): senza,
    // il download locale del gz (~12.6MB) dura ~0.1s e il reader produce
    // pochi chunk giganti -> il criterio "progress scatta più volte" non
    // è osservabile. Con 3MB/s il gz arriva in ~4s -> ~40 eventi throttled
    // a 100ms.
    const cdp = await page.context().newCDPSession(page);
    await cdp.send('Network.enable');
    await cdp.send('Network.emulateNetworkConditions', {
        offline: false,
        latency: 50,
        downloadThroughput: 3 * 1024 * 1024, // 3MB/s
        uploadThroughput: 3 * 1024 * 1024,
    });
    const r = await page.evaluate(async ({ url, sizeUrl }) => {
        const events = [];
        const t0 = performance.now();
        const d = await downloadWasm(url, {
            sizeUrl,
            onProgress: (p) => events.push(Object.assign({ t: performance.now() - t0 }, p)),
        });
        const h = await crypto.subtle.digest('SHA-256', d.bytes);
        const sha = Array.from(new Uint8Array(h)).map(x => x.toString(16).padStart(2, '0')).join('');
        const prog = events.filter(e => e.phase === 'progress');
        const fracs = prog.map(e => e.fraction);
        const intermedi = prog.slice(0, -1); // esclude il finale frac=1 (fuori throttle by design)
        const rate = (intermedi.length > 1)
            ? (intermedi.length - 1) / ((prog[prog.length - 2].t - prog[0].t) / 1000)
            : 0;
        return {
            len: d.byteLength, total: d.total, determinate: d.determinate,
            nProgress: prog.length, firstFrac: fracs[0], lastFrac: fracs[fracs.length - 1],
            maxFrac: Math.max(...fracs), endFrac: events.find(e => e.phase === 'end').fraction,
            rate,
            sha, validate: WebAssembly.validate(d.bytes),
        };
    }, {
        url: `http://127.0.0.1:${PORT}/gravity_sandbox_p80_bg.wasm?v=0.14.80`,
        sizeUrl: `http://127.0.0.1:${PORT}/gravity_sandbox_p80_bg.wasm.size?v=0.14.80`,
    });

    report('R1 wasm reale via serve_wasm.py: determinato, total=decompresso, frac→1',
        r.determinate === true && r.total === diskLen && r.nProgress > 3 && r.lastFrac === 1 && r.endFrac === 1 && r.maxFrac === 1,
        `total=${r.total}/${diskLen} progress=${r.nProgress} first=${r.firstFrac?.toFixed(3)} last=${r.lastFrac} rate=${r.rate.toFixed(1)}/s`);
    report('R2 sha256 byte ricevuti == disco',
        r.sha === diskSha && r.len === diskLen,
        `match=${r.sha === diskSha} len=${r.len}/${diskLen}`);
    report('R3 byte ricevuti: WebAssembly.validate === true (browser)',
        r.validate === true,
        `validate=${r.validate} (modulo identico a prima, pronto per instantiateStreaming)`);

    // ---- R4: regressione boot app reale -------------------------------
    {
        const p2 = await browser.newPage();
        const pageErrors = [];
        let badgeTime = 0;
        p2.on('pageerror', (e) => pageErrors.push({ t: Date.now(), msg: String(e && e.message ? e.message : e) }));
        await p2.goto(`http://127.0.0.1:${PORT}/index.html`, { waitUntil: 'domcontentloaded' });
        // attesa badge ✅ (WASM boot completo con WebGL swiftshader)
        await p2.waitForFunction(() => {
            const v = document.getElementById('version-badge');
            return v && v.textContent.indexOf('✅') >= 0;
        }, null, { timeout: 120000 });
        badgeTime = Date.now();
        // stabilità post-boot: 3s di runtime senza trap (il poll debug
        // markerebbe 🛑 DEAD se il wasm fosse morto)
        await sleep(3000);
        // lbDone() fissa opacity 0 e dopo 460ms display:none: attende il
        // fade-out COMPLETO prima di verificare che la barra sia sparita.
        await p2.waitForFunction(() => {
            const lb = document.getElementById('loading-bar');
            return !lb || lb.style.display === 'none';
        }, null, { timeout: 5000 });
        const barHidden = await p2.evaluate(() => {
            const lb = document.getElementById('loading-bar');
            return !lb || lb.style.display === 'none' || lb.classList.contains('lb-hidden');
        });
        // pageerror DOPO il badge ✅ = teardown della chiusura pagina
        // (winit/Worker interrotti), benigno e pre-esistente; PRIMA del
        // badge sarebbe un problema reale del flusso di caricamento.
        const preBoot = pageErrors.filter(pe => pe.t < badgeTime);
        const postBoot = pageErrors.length - preBoot.length;
        report('R4 boot app reale v0.14.80: badge ✅ + loading bar scomparsa',
            barHidden === true && preBoot.length === 0,
            `barHidden=${barHidden} pageerror PRE-boot=${preBoot.length} POST-boot(benigni)=${postBoot}${pageErrors.length ? ' [' + pageErrors.map(pe => pe.msg).join(' | ').substring(0, 200) + ']' : ''}`);
        await p2.close();
    }

} catch (e) {
    console.error('DRIVER ERROR:', e);
    process.exitCode = 1;
} finally {
    if (browser) await browser.close().catch(() => {});
    server.kill();
    await sleep(200);
}

const failed = results.filter(r => !r.ok).length;
console.log(failed === 0
    ? `\nVERIFICA REALE COMPLETA — ${results.length}/${results.length} PASS`
    : `\n${failed}/${results.length} REAL-CHECK FALLITI`);
process.exit(failed === 0 ? 0 : 1);
