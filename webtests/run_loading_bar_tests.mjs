// webtests/run_loading_bar_tests.mjs — driver dei criteri di accettazione
// della card t_63406e55 (componente UI barra caricamento a due fasi).
// Avvia il fixture server dedicato, apre la pagina host in Chromium headless
// (playwright-core) e guida il componente con eventi SIMULATI (nessun loader
// reale: la card lo vieta esplicitamente), ispezionando DOM/CSS/ARIA.
//
// CRITERIO 1 — demo/test con eventi simulati mostra le due fasi:
//   A1 happy path simulato: download determinato (label % + byte tipo
//      'Download… 42% · 3.2 MB / 8.1 MB', fill width = fraction*100) →
//      fase compile (classe lb-indeterminate, label 'Compilazione…',
//      fill NON ha width inline — lo gestisce il CSS dell'animazione).
//   A2 la sequenza 12 update progressivi cresce MONOTONA (width e %).
//
// CRITERIO 2 — modalità indeterminata SENZA percentuale:
//   B1 fraction null (o payload {fraction:null,total:null}) → classe
//      lb-indeterminate, aria-valuenow ASSENTE, label SENZA '%'.
//   B2 'compile' dopo download parziale: stesse garanzie (niente %,
//      niente valuenow) + la label non riporta i byte del download.
//
// CRITERIO 3 — NESSUN layout shift quando la barra scompare:
//   C1 ready → dopo il fade (opacity 0 + visibility hidden) la barra NON
//      occupa display:none... verifica: getBoundingClientRect del root e di
//      un marker SOTTO la barra sono INVARIATI tra 'prima di done' e
//      'dopo done+fade'. pointer-events none dopo il fade.
//   C2 (corollario auto): ready prima del fade = solo opacity cambia.
//
// CRITERIO 4 — ARIA presente e corretto:
//   D1 track role=progressbar, aria-valuemin=0, aria-valuemax=100,
//      aria-label='Caricamento del sandbox'.
//   D2 determinato: aria-valuenow = % intera (es. 42), aria-valuetext
//      con % + byte ('42% · 3.2 MB / 8.1 MB').
//   D3 indeterminato: aria-valuenow RIMOSSO, aria-valuetext = label fase.
//   D4 error: label 'Errore: …', niente valuenow; ready: valuenow=100.
//
// EXTRA — robustezza d'uso:
//   E1 clamp: fraction 1.3 (o loaded>total) → MAI >100% (fill ≤ 100%,
//      aria-valuenow ≤ 100, label '100%'): il bug storico del 130% non
//      può ricapitare nella UI.
//   E2 retry: error('msg', handler) → bottone 'Riprova' VISIBILE e con
//      pointer-events; click → handler chiamato 1 volta, UI passa a
//      'Riprovo…' indeterminato; senza handler → NESSUN bottone morto.
//   E3 API contratto: update('ready') ≡ done(); formatBytes unit test.
//   E4 demo page: la demo interattiva monta e il primo scenario parte
//      da sola (auto-play happy path) — smoke test.
import { spawn } from 'node:child_process';
import { setTimeout as sleep } from 'node:timers/promises';

const PW = '/home/ubuntu/workspace/napkin-maker/node_modules/playwright-core/index.js';
const CHROME = '/home/ubuntu/.cache/ms-playwright/chromium-1234/chrome-linux/chrome';
const PORT = 8185;
const BASE = `http://127.0.0.1:${PORT}`;

const results = [];
function report(name, ok, detail) {
    results.push({ name, ok, detail });
    console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`);
}

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
    const page = await browser.newPage();
    await page.goto(`${BASE}/loading-bar-test-page.html`);
    await page.waitForFunction(() => window.__lb && typeof window.__lb.createLoadingBar === 'function', null, { timeout: 10000 });

    // helper: crea la barra dentro la pagina e ne espone refs + state
    const mk = () => page.evaluate(() => {
        const host = document.createElement('div');
        host.id = 'host';
        document.body.appendChild(host);
        const bar = window.__lb.createLoadingBar({ mount: host });
        window.__bar = bar;
        return true;
    });

    // helper: snapshot DOM della barra (classi, label, fill, ARIA, visibility)
    const snap = () => page.evaluate(() => {
        const b = window.__bar;
        const r = b.refs.root;
        const cs = getComputedStyle(r);
        return {
            cls: r.className,
            label: b.refs.label.textContent,
            fillWidth: b.refs.fill.style.width,
            fillComputedWidth: getComputedStyle(b.refs.fill).width,
            role: b.refs.track.getAttribute('role'),
            amin: b.refs.track.getAttribute('aria-valuemin'),
            amax: b.refs.track.getAttribute('aria-valuemax'),
            anow: b.refs.track.getAttribute('aria-valuenow'),
            atext: b.refs.track.getAttribute('aria-valuetext'),
            alabel: b.refs.track.getAttribute('aria-label'),
            rect: r.getBoundingClientRect().toJSON(),
            display: cs.display,
            visibility: cs.visibility,
            opacity: cs.opacity,
            pointerEvents: cs.pointerEvents,
            retryVisible: getComputedStyle(b.refs.retryRow).display !== 'none',
            state: b.state,
        };
    });

    const TOTAL = 51_358_364;

    // ===== CRITERIO 1 + 4 (download determinato) =========================
    await mk();
    {
        // sequenza simulata di 'download-progress' (payload wasm_runtime.js)
        await page.evaluate((TOTAL) => {
            window.__bar.update('download', { loaded: 0, total: TOTAL, fraction: 0 });
            window.__bar.update('download', { loaded: Math.round(TOTAL * 0.42), total: TOTAL, fraction: 0.42 });
        }, TOTAL);
        let s = await snap();
        report('A1a download determinato: label con % e byte',
            s.label === 'Download… 42% · 21.6 MB / 51.4 MB',
            `label="${s.label}"`);
        report('A1b fill width = fraction*100 (42%)',
            s.fillWidth === '42%', `width="${s.fillWidth}"`);
        report('A1c non indeterminato (nessuna classe lb-indeterminate)',
            !s.cls.includes('lb-indeterminate'), `cls="${s.cls}"`);
        report('D1 ARIA base: role=min/max/label',
            s.role === 'progressbar' && s.amin === '0' && s.amax === '100' && s.alabel === 'Caricamento del sandbox',
            `role=${s.role} min=${s.amin} max=${s.amax} label="${s.alabel}"`);
        report('D2 determinato: aria-valuenow=42, aria-valuetext="42% · 21.6 MB / 51.4 MB"',
            s.anow === '42' && s.atext === '42% · 21.6 MB / 51.4 MB',
            `now=${s.anow} text="${s.atext}"`);

        // sequenza crescente 12 step: % e width MONOTONE
        const seq = await page.evaluate((TOTAL) => {
            const out = [];
            for (let i = 1; i <= 12; i++) {
                const f = i / 12;
                window.__bar.update('download', { loaded: Math.round(TOTAL * f), total: TOTAL, fraction: f });
                const b = window.__bar;
                out.push({
                    label: b.refs.label.textContent,
                    width: b.refs.fill.style.width,
                    now: b.refs.track.getAttribute('aria-valuenow'),
                });
            }
            return out;
        }, TOTAL);
        const pcts = seq.map(x => parseInt(x.now, 10));
        const widths = seq.map(x => parseFloat(x.width));
        const monoP = pcts.every((p, i) => i === 0 || p >= pcts[i - 1]);
        const monoW = widths.every((w, i) => i === 0 || w >= widths[i - 1]);
        report('A2 sequenza 12 step: % e width crescenti monotone fino a 100',
            monoP && monoW && pcts[0] > 0 && pcts[pcts.length - 1] === 100 && widths[widths.length - 1] === 100,
            `pcts ${pcts[0]}..${pcts[pcts.length - 1]} monoP=${monoP} monoW=${monoW}`);

        // ===== fase compile: transizione a indeterminato ================
        await page.evaluate(() => window.__bar.update('compile'));
        s = await snap();
        report('A3 transizione compile: classe lb-indeterminate ON, label "Compilazione…"',
            s.cls.includes('lb-indeterminate') && s.label === 'Compilazione…',
            `cls="${s.cls}" label="${s.label}"`);
        report('A4 compile: fill senza width inline (l\'animazione CSS gestisce il 40%)',
            s.fillWidth === '', `width="${s.fillWidth}"`);
        report('B2 compile dopo download: NESSUN valuenow, NESSUN byte in label',
            s.anow === null && !s.label.includes('/') && !s.label.includes('%'),
            `now=${s.anow} label="${s.label}"`);

        // ===== CRITERIO 3: done → fade → NESSUN layout shift ============
        // marker sotto la barra per verificare che nulla si sposti
        await page.evaluate(() => {
            const m = document.createElement('div');
            m.id = 'below-marker';
            m.style.cssText = 'position:absolute;top:60%;left:0;width:40px;height:10px;';
            document.body.appendChild(m);
        });
        const before = await snap();
        const markerBefore = await page.evaluate(() => document.getElementById('below-marker').getBoundingClientRect().toJSON());
        await page.evaluate(() => window.__bar.update('ready'));
        await sleep(200); // a metà fade: SOLO opacity cambia (C2)
        const mid = await snap();
        report('C2 durante il fade (opacity < 1) il box è ancora nel layout',
            mid.display !== 'none' && mid.visibility !== 'hidden' && parseFloat(mid.opacity) < 1,
            `display=${mid.display} visibility=${mid.visibility} opacity=${mid.opacity}`);
        await sleep(400); // fade completato (450ms) + visibility hidden
        const after = await snap();
        const markerAfter = await page.evaluate(() => document.getElementById('below-marker').getBoundingClientRect().toJSON());
        report('C1 dopo ready+fade: rect barra INVARIATO (nessun layout shift)',
            JSON.stringify(before.rect) === JSON.stringify(after.rect),
            `before=${JSON.stringify(before.rect)} after=${JSON.stringify(after.rect)}`);
        report('C1b marker sotto la barra INVARIATO',
            JSON.stringify(markerBefore) === JSON.stringify(markerAfter),
            `y ${markerBefore.y} -> ${markerAfter.y}`);
        report('C1c dopo il fade: visibility=hidden e pointer-events=none',
            after.visibility === 'hidden' && after.pointerEvents === 'none',
            `visibility=${after.visibility} pe=${after.pointerEvents}`);
        report('D4 ready: aria-valuenow=100',
            after.anow === '100', `now=${after.anow}`);
    }

    // ===== CRITERIO 2: indeterminato SENZA percentuale ===================
    {
        // nuova istanza pulita
        await page.evaluate(() => { window.__bar.destroy(); window.__bar = window.__lb.createLoadingBar({ mount: '#host' }); });
        // download indeterminato: fraction null (chunked/gzip senza totale)
        await page.evaluate((TOTAL) => window.__bar.update('download', { loaded: 0, total: null, fraction: null }), TOTAL);
        let s = await snap();
        report('B1a download indeterminato: classe ON, label SENZA %',
            s.cls.includes('lb-indeterminate') && !s.label.includes('%') && s.label === 'Download…',
            `cls="${s.cls}" label="${s.label}"`);
        report('B1b indeterminato: aria-valuenow ASSENTE, aria-valuetext = label',
            s.anow === null && s.atext === s.label,
            `now=${s.anow} text="${s.atext}" label="${s.label}"`);
        // fraction null anche a metà download (loaded cresce, % mai inventata)
        await page.evaluate((TOTAL) => window.__bar.update('download', { loaded: Math.round(TOTAL * 0.6), total: null, fraction: null }), TOTAL);
        s = await snap();
        report('B1c indeterminato a metà download: NESSUNA % inventata (loaded visibile senza / totale)',
            s.anow === null && !s.label.includes('%') && s.label.includes('MB'),
            `now=${s.anow} label="${s.label}"`);
    }

    // ===== errore + retry ================================================
    {
        await page.evaluate(() => { window.__bar.destroy(); window.__bar = window.__lb.createLoadingBar({ mount: '#host' }); });
        await page.evaluate((TOTAL) => window.__bar.update('download', { loaded: Math.round(TOTAL * 0.3), total: TOTAL, fraction: 0.3 }), TOTAL);
        // errore con handler retry
        await page.evaluate(() => {
            window.__retryCalled = 0;
            window.__bar.error('wasm fetch HTTP 404', () => { window.__retryCalled++; });
        });
        let s = await snap();
        report('E2a error: label "Errore: wasm fetch HTTP 404", classe lb-error',
            s.cls.includes('lb-error') && s.label === 'Errore: wasm fetch HTTP 404',
            `cls="${s.cls}" label="${s.label}"`);
        report('E2b error: bottone Riprova VISIBILE, fill 100%',
            s.retryVisible === true && s.fillWidth === '100%',
            `retryVisible=${s.retryVisible} fill="${s.fillWidth}"`);
        report('D4b error: NESSUN aria-valuenow (la % non c\'entra)',
            s.anow === null, `now=${s.anow}`);
        // click retry: handler chiamato 1 volta, UI a 'Riprovo…' indeterminato
        await page.click('.lb-retry');
        s = await snap();
        const calls = await page.evaluate(() => window.__retryCalled);
        report('E2c click Riprova: handler chiamato ESATTAMENTE 1 volta',
            calls === 1, `calls=${calls}`);
        report('E2d dopo il click: fase "Riprovo…" indeterminata',
            s.cls.includes('lb-indeterminate') && !s.cls.includes('lb-error') && s.label === 'Riprovo…',
            `cls="${s.cls}" label="${s.label}"`);
        // senza handler: NESSUN bottone morto
        await page.evaluate(() => window.__bar.error('errore finale'));
        s = await snap();
        report('E2e errore SENZA handler: bottone retry NASCOSTO',
            s.retryVisible === false, `retryVisible=${s.retryVisible}`);
    }

    // ===== EXTRA: clamp, contratto API, formatBytes ======================
    {
        await page.evaluate(() => { window.__bar.destroy(); window.__bar = window.__lb.createLoadingBar({ mount: '#host' }); });
        // clamp: fraction 1.3 → mai oltre il 100%
        await page.evaluate((TOTAL) => window.__bar.update('download', { loaded: 66_666_873, total: TOTAL, fraction: 1.3 }), TOTAL);
        let s = await snap();
        report('E1a clamp fraction 1.3: fill/aria/label FERMI a 100%',
            s.fillWidth === '100%' && s.anow === '100' && s.label.includes('100%'),
            `fill="${s.fillWidth}" now=${s.anow} label="${s.label}"`);
        // clamp: loaded > total (header fuori sincrono) senza fraction
        await page.evaluate((TOTAL) => window.__bar.update('download', { loaded: TOTAL * 2, total: TOTAL }), TOTAL);
        s = await snap();
        report('E1b clamp loaded>total: mai >100% (il bug storico del 130% è morto in UI)',
            s.fillWidth === '100%' && s.anow === '100',
            `fill="${s.fillWidth}" now=${s.anow}`);
        // update('ready') ≡ done()
        await page.evaluate(() => window.__bar.update('ready'));
        s = await snap();
        report('E3a update("ready") ≡ done(): fase ready + fade in corso',
            s.state.phase === 'ready' && s.state.done === true && s.cls.includes('lb-done'),
            `phase=${s.state.phase} done=${s.state.done}`);
        // formatBytes: unit test dei casi limite (esempio del task: '3.2 MB')
        const fb = await page.evaluate(() => ({
            a: window.__lb.formatBytes(3_200_000),
            b: window.__lb.formatBytes(8_100_000),
            c: window.__lb.formatBytes(0),
            d: window.__lb.formatBytes(null),
            e: window.__lb.formatBytes(-5),
            f: window.__lb.formatBytes(999),
            g: window.__lb.formatBytes(512),
            h: window.__lb.formatBytes(1_000_000_000),
            i: window.__lb.formatBytes(51_358_364),
        }));
        report('E3b formatBytes: 3.2 MB / 8.1 MB (esempio del task) + edge case',
            fb.a === '3.2 MB' && fb.b === '8.1 MB' && fb.c === '0 B' && fb.d === '' && fb.e === '' &&
            fb.f === '999 B' && fb.g === '512 B' && fb.h === '1 GB' && fb.i === '51.4 MB',
            JSON.stringify(fb));
        // destroy rimuove il DOM
        const destroyed = await page.evaluate(() => {
            window.__bar.destroy();
            return !document.querySelector('.lb-root');
        });
        report('E3c destroy(): DOM dell\'istanza rimosso', destroyed === true);
    }

    // ===== S: coerenza stile coi token di #loading-bar in index.html =====
    // Il componente inietta il proprio CSS: verifichiamo coi computed styles
    // che i token visivi siano quelli del progetto (pannello blur scuro,
    // track 6px, fill blu 100,120,255, animazione indeterminata, rosso errore).
    {
        await page.evaluate(() => { window.__bar.destroy(); window.__bar = window.__lb.createLoadingBar({ mount: '#host' }); });
        const st = await page.evaluate(() => {
            const r = window.__bar.refs.root;
            const out = { styleInjected: !!document.getElementById('lb-component-styles') };
            const panel = getComputedStyle(r.querySelector('.lb-panel'));
            out.panelBg = panel.backgroundColor;
            out.panelRadius = panel.borderRadius;
            out.panelBorder = panel.borderColor;
            const label = getComputedStyle(r.querySelector('.lb-label'));
            out.labelColor = label.color;
            out.labelSize = label.fontSize;
            const track = getComputedStyle(r.querySelector('.lb-track'));
            out.trackH = track.height;
            out.trackRadius = track.borderRadius;
            const fill = getComputedStyle(r.querySelector('.lb-fill'));
            out.fillBg = fill.backgroundColor;
            // indeterminato: animazione attiva
            window.__bar.update('compile');
            out.animName = getComputedStyle(r.querySelector('.lb-fill')).animationName;
            out.animDur = getComputedStyle(r.querySelector('.lb-fill')).animationDuration;
            out.animIter = getComputedStyle(r.querySelector('.lb-fill')).animationIterationCount;
            // errore: rosso
            window.__bar.error('x');
            out.errFill = getComputedStyle(r.querySelector('.lb-fill')).backgroundColor;
            out.errBorder = getComputedStyle(r.querySelector('.lb-panel')).borderColor;
            out.errAnim = getComputedStyle(r.querySelector('.lb-fill')).animationName;
            return out;
        });
        report('S1 stile: <style> iniettato una sola volta con id dedicato',
            st.styleInjected === true, `injected=${st.styleInjected}`);
        report('S2 stile: pannello rgba(12,12,24,0.8), raggio 6px, bordo 10% bianco',
            st.panelBg === 'rgba(12, 12, 24, 0.8)' && st.panelRadius === '6px' && st.panelBorder === 'rgba(255, 255, 255, 0.1)',
            `bg=${st.panelBg} radius=${st.panelRadius} border=${st.panelBorder}`);
        report('S3 stile: label 13px, track 6px raggio 3px, fill blu rgba(100,120,255,0.85)',
            st.labelSize === '13px' && st.trackH === '6px' && st.trackRadius === '3px' && st.fillBg === 'rgba(100, 120, 255, 0.85)',
            `label=${st.labelSize} track=${st.trackH}/${st.trackRadius} fill=${st.fillBg}`);
        report('S4 stile: indeterminato animato (lb-slide 1.1s infinite)',
            st.animName === 'lb-slide' && st.animDur === '1.1s' && st.animIter === 'infinite',
            `anim=${st.animName} ${st.animDur} ${st.animIter}`);
        report('S5 stile: errore rosso (fill rgba(255,80,80,0.85), bordo, animazione OFF)',
            st.errFill === 'rgba(255, 80, 80, 0.85)' && st.errBorder === 'rgba(255, 80, 80, 0.55)' && st.errAnim === 'none',
            `fill=${st.errFill} border=${st.errBorder} anim=${st.errAnim}`);
        await page.evaluate(() => window.__bar.destroy());
    }

    // ===== E4: demo page smoke test =======================================
    {
        const demo = await browser.newPage();
        const demoLogs = [];
        demo.on('console', (m) => demoLogs.push(m.text()));
        await demo.goto(`${BASE}/loading-bar-demo.html`);
        await demo.waitForFunction(() => document.querySelector('.lb-root'), null, { timeout: 10000 });
        // la demo parte DA SOLA con l'happy path: attende la fase compile
        await demo.waitForFunction(() => {
            const r = document.querySelector('.lb-root');
            return r && r.className.includes('lb-indeterminate') && document.querySelector('.lb-label').textContent.includes('Compilazione');
        }, null, { timeout: 15000 });
        const demoState = await demo.evaluate(() => ({
            label: document.querySelector('.lb-label').textContent,
            cls: document.querySelector('.lb-root').className,
        }));
        report('E4 demo: auto-play happy path → fase compile animata raggiunta',
            demoState.cls.includes('lb-indeterminate') && demoState.label === 'Compilazione…',
            `cls="${demoState.cls}" label="${demoState.label}"`);
        // e arriva a ready con fade-out (marker layout fermo)
        const marker1 = await demo.evaluate(() => document.getElementById('shift-marker').getBoundingClientRect().toJSON());
        await demo.waitForFunction(() => {
            const r = document.querySelector('.lb-root');
            return r && r.className.includes('lb-done');
        }, null, { timeout: 15000 });
        await sleep(600);
        const marker2 = await demo.evaluate(() => document.getElementById('shift-marker').getBoundingClientRect().toJSON());
        const barHidden = await demo.evaluate(() => {
            const r = document.querySelector('.lb-root');
            return getComputedStyle(r).visibility === 'hidden' && getComputedStyle(r).opacity === '0';
        });
        report('E4b demo: ready → fade-out, marker layout FERMO',
            barHidden && JSON.stringify(marker1) === JSON.stringify(marker2),
            `barHidden=${barHidden} marker y ${marker1.y}->${marker2.y}`);
        await demo.close();
    }
} catch (e) {
    console.error('DRIVER ERROR:', e);
    process.exitCode = 1;
} finally {
    if (browser) await browser.close().catch(() => {});
    fetch(`${BASE}/shutdown`, { method: 'POST' }).catch(() => {});
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
