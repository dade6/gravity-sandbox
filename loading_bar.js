// loading_bar.js — Componente UI riutilizzabile: barra di caricamento a
// DUE FASI per il sandbox (card t_63406e55).
//
// FASI:
//   1. download — DETERMINATO: frazione 0..1 -> riempimento 0-100% con
//      etichetta percentuale + byte (es. 'Download… 42% · 3.2 MB / 8.1 MB').
//      Se la taglia totale non è conoscibile (chunked, gzip senza header/sidecar)
//      passa in modalità INDENERMINATA: colonna animata, MAI percentuale finta.
//   2. compile — SEMPRE indeterminato: la compilazione WebAssembly non
//      offre progressi nativi, quindi è una FASE ('Compilazione…'), non una %
//      inventata. Transizione automatica: la barra passa dall'riempimento
//      determinato allo scorrimento animato.
//
// STATI EXTRA:
//   error  — messaggio di errore + pulsante 'Riprova' (visibile SOLO se c'è
//            un handler retry: un bottone morto è peggio di nessun bottone).
//            Pannello/fill in rosso, coerenti coi token di #error-details.
//   ready  — fade-out (opacity 0 in 450ms) POI visibility:hidden: il box
//            resta nel layout -> NESSUN LAYOUT SHIFT quando scompare.
//            Mai display:none: toglierebbe il box e farebbe reflow.
//
// ACCESSIBILITÀ (criterio 4):
//   - track con role="progressbar", aria-valuemin="0", aria-valuemax="100"
//   - aria-valuenow = % intera SOLO in modalità determinata (rimosso quando
//     indeterminato: è COSÌ che si segnala lo stato indeterminato in ARIA)
//   - aria-valuetext con % + byte ('42% · 3.2 MB / 8.1 MB') o testo di fase
//   - aria-label costante ('Caricamento del sandbox', sovrascrivibile)
//   - root con aria-live="polite": annuncia i cambi di fase/label
//     (stesso pattern dell'#loading-bar esistente in index.html)
//
// STILE: riprende ESATTAMENTE i token visivi dell'#loading-bar di index.html
// (pannello rgba(12,12,24,0.8) + blur 6px, bordo rgba(255,255,255,0.10),
// raggio 6px, label bianco/0.6 13px, track 6px, fill rgba(100,120,255,0.85),
// errore rosso rgba(255,80,80,…), bottone retry coi token di #copy-err).
// Il CSS è iniettato una sola volta (dedup per id) come <style>.
//
// USO:
//   import { createLoadingBar } from './loading_bar.js';
//   const bar = createLoadingBar({ mount: '#ui-overlay' });
//   bar.update('download', { loaded, total, fraction }); // 0..1 o null
//   bar.update('compile');                              // indeterminato
//   bar.update('ready');                                // fade-out
//   bar.error('wasm fetch HTTP 404', () => initWithRetry());
//
// INTEGRAZIONE (card t_8553586d): update() accetta DIRETTAMENTE i payload
// degli eventi di wasm_runtime.js:
//   events.on('download-start',   p => bar.update('download', p));
//   events.on('download-progress',p => bar.update('download', p));
//   events.on('compile-start',    () => bar.update('compile'));
//   events.on('ready',            () => bar.update('ready'));
//   events.on('error',            e => bar.error(e.message, () => retry()));
// 'progress' può essere: numero 0..1 | null | {fraction, loaded, total}.
// La frazione è CLAMPATA a [0,1]: anche un totale fuori sincrono non può
// mostrare il 130% storico.
//
// Il componente NON si collega al loader reale (compito della card di
// integrazione): è verificato con eventi simulati (webtests/run_loading_bar_tests.mjs).

// ---- utilità -----------------------------------------------------------

// Formattazione byte per l'etichetta: '3.2 MB / 8.1 MB' (esempio del task).
// Base 1000 (il progetto chiama "51MB" il wasm da 51.358.364 byte), 1 decimale
// sotto i 100, intero sopra. null/garbage -> stringa vuota (mai 'NaN').
export function formatBytes(n) {
    if (n == null || !Number.isFinite(n) || n < 0) return '';
    if (n < 1000) return Math.round(n) + ' B';
    const units = ['KB', 'MB', 'GB', 'TB'];
    let v = n;
    let i = -1;
    do { v /= 1000; i++; } while (v >= 1000 && i < units.length - 1);
    const s = v >= 100 ? String(Math.round(v)) : v.toFixed(1).replace(/\.0$/, '');
    return s + ' ' + units[i];
}

// Sezione byte dell'etichetta: '3.2 MB / 8.1 MB' con totale, sola taglia
// scaricata quando significativa (>0), vuota altrimenti (mai '0 B' inutile
// né 'NaN').
function bytesPart(loaded, total) {
    if (loaded != null && total != null) return formatBytes(loaded) + ' / ' + formatBytes(total);
    if (loaded != null && loaded > 0) return formatBytes(loaded);
    return '';
}

// Etichetta composta: '<base> <pct>% · <bytes>' — il separatore '·' solo
// quando c'è la %; senza % i byte seguono la base con uno spazio.
// Senza base: '42% · 3.2 MB / 8.1 MB' o '1.2 MB'.
function composeLabel(base, pct, bytes) {
    let s = base != null && base !== '' ? String(base) : '';
    if (pct != null) s += (s ? ' ' : '') + pct + '%';
    if (bytes) s += (s ? (pct != null ? ' · ' : ' ') : '') + bytes;
    return s;
}

// Inietta il CSS del componente una sola volta (più istanze = stesso style).
const STYLE_ID = 'lb-component-styles';
function ensureStyles() {
    if (document.getElementById(STYLE_ID)) return;
    const style = document.createElement('style');
    style.id = STYLE_ID;
    style.textContent = `
.lb-root {
    position: absolute;
    top: 50%; left: 50%;
    width: 300px;
    transform: translate(-50%, -50%);
    pointer-events: auto;
    transition: opacity 0.45s ease;
    z-index: 1;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
}
.lb-root .lb-panel {
    background: rgba(12, 12, 24, 0.8);
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.10);
    padding: 14px 16px;
}
.lb-root .lb-label {
    color: rgba(255, 255, 255, 0.6);
    font-size: 13px;
    line-height: 1.4;
    margin-bottom: 10px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: 0.2px;
}
.lb-root .lb-track {
    position: relative;
    height: 6px;
    border-radius: 3px;
    background: rgba(255, 255, 255, 0.12);
    overflow: hidden;
}
.lb-root .lb-fill {
    position: absolute;
    top: 0; left: 0;
    height: 100%;
    width: 0%;
    background: rgba(100, 120, 255, 0.85);
    border-radius: 3px;
    transition: width 0.1s ease-out;
}
/* Indeterminato (Compilazione… / Riprovo… / download senza taglia):
   colonna che scorre nel track. FASE, non percentuale finta. */
.lb-root.lb-indeterminate .lb-fill {
    width: 40%;
    animation: lb-slide 1.1s ease-in-out infinite;
}
@keyframes lb-slide {
    0%   { transform: translateX(-100%); }
    100% { transform: translateX(250%); }
}
/* Errore: pannello/fill/label rossi (token di #error-details). */
.lb-root.lb-error .lb-panel { border-color: rgba(255, 80, 80, 0.55); }
.lb-root.lb-error .lb-fill {
    background: rgba(255, 80, 80, 0.85);
    animation: none;
}
.lb-root.lb-error .lb-label { color: #ffb4b4; }
/* App pronta: fade out dell'opacità; il box resta (visibility gestita da JS
   dopo la transizione -> nessun layout shift). */
.lb-root.lb-done { opacity: 0; }
/* Inizialmente nascosta: la prima update()/show() la rivela. */
.lb-root.lb-hidden { display: none; }
/* Riga retry: presente SOLO nello stato error E con handler registrato. */
.lb-root .lb-retry-row { display: none; margin-top: 10px; }
.lb-root.lb-error.lb-retryable .lb-retry-row { display: block; }
.lb-root .lb-retry {
    height: 30px;
    padding: 0 12px;
    border: 1px solid rgba(255, 120, 120, 0.6);
    border-radius: 6px;
    background: rgba(255, 80, 80, 0.25);
    color: #ffd7d7;
    font-size: 13px;
    cursor: pointer;
    white-space: nowrap;
    transition: background 0.15s, color 0.15s;
}
.lb-root .lb-retry:hover { background: rgba(255, 80, 80, 0.45); color: #fff; }
.lb-root .lb-retry:active { background: rgba(255, 80, 80, 0.55); }
`;
    document.head.appendChild(style);
}

// Etichette di default (italiano, come il resto della UI del progetto).
export const DEFAULT_LABELS = {
    download: 'Download…',
    compile: 'Compilazione…',
    retrying: 'Riprovo…',
    ready: 'Pronto!',
    errorPrefix: 'Errore: ',
    retry: 'Riprova',
};
export const DEFAULT_ARIA_LABEL = 'Caricamento del sandbox';
export const DEFAULT_FADE_MS = 450;

// ---- componente --------------------------------------------------------

export function createLoadingBar(opts = {}) {
    ensureStyles();

    const labels = Object.assign({}, DEFAULT_LABELS, opts.labels || {});
    const ariaLabel = opts.ariaLabel || DEFAULT_ARIA_LABEL;
    const fadeMs = opts.fadeMs != null ? opts.fadeMs : DEFAULT_FADE_MS;

    // mount: Element | selettore stringa | null -> document.body
    let mount = opts.mount || null;
    if (typeof mount === 'string') {
        const found = document.querySelector(mount);
        if (!found) throw new Error('createLoadingBar: mount non trovato: ' + mount);
        mount = found;
    }
    if (!mount) mount = document.body;

    // ---- DOM (struttura identica all'#loading-bar di index.html + retry) ----
    const root = document.createElement('div');
    root.className = 'lb-root lb-hidden';
    root.setAttribute('aria-live', 'polite');
    if (opts.width != null) root.style.width = opts.width + 'px';
    if (opts.zIndex != null) root.style.zIndex = String(opts.zIndex);
    if (opts.fadeMs != null) root.style.transitionDuration = fadeMs + 'ms';

    const panel = document.createElement('div');
    panel.className = 'lb-panel';

    const labelEl = document.createElement('div');
    labelEl.className = 'lb-label';
    labelEl.textContent = labels.download;

    const track = document.createElement('div');
    track.className = 'lb-track';
    track.setAttribute('role', 'progressbar');
    track.setAttribute('aria-valuemin', '0');
    track.setAttribute('aria-valuemax', '100');
    track.setAttribute('aria-label', ariaLabel);

    const fill = document.createElement('div');
    fill.className = 'lb-fill';

    const retryRow = document.createElement('div');
    retryRow.className = 'lb-retry-row';
    const retryBtn = document.createElement('button');
    retryBtn.type = 'button';
    retryBtn.className = 'lb-retry';
    retryBtn.textContent = labels.retry;

    track.appendChild(fill);
    retryRow.appendChild(retryBtn);
    panel.appendChild(labelEl);
    panel.appendChild(track);
    panel.appendChild(retryRow);
    root.appendChild(panel);
    mount.appendChild(root);

    // ---- stato interno ---------------------------------------------------
    const state = {
        phase: 'idle',     // 'idle' | 'download' | 'compile' | 'retrying' | …
        fraction: null,    // 0..1 in determinato, null indeterminato
        label: labels.download,
        hidden: true,
        done: false,
        error: null,
    };
    // Handler retry: priorità per-call > default di creazione. error(msg)
    // SENZA handler torna al default di opts.onRetry (o niente) — MAI resta
    // attaccato l'handler di un error precedente (un bottone 'Riprova' che
    // riprova una cosa che il chiamante non ha più proposto è un bug UX).
    const baseRetryFn = typeof opts.onRetry === 'function' ? opts.onRetry : null;
    let retryFn = baseRetryFn;
    let fadeTimer = null;

    // ARIA: valuenow solo quando determinato; valuetext sempre informativo.
    function setAria(pct, valuetext) {
        if (pct != null) track.setAttribute('aria-valuenow', String(pct));
        else track.removeAttribute('aria-valuenow');
        if (valuetext != null && valuetext !== '') track.setAttribute('aria-valuetext', valuetext);
        else track.removeAttribute('aria-valuetext');
    }

    // ---- API -------------------------------------------------------------

    // update(phase, progress, label)
    //   phase   'download' | 'compile' | 'retrying' | qualsiasi stringa
    //           ('ready' = scorciatoia per done()). 'compile' forza sempre
    //           l'indeterminato (mai % finta sulla compilazione).
    //   progress numero 0..1 | null | {fraction, loaded, total} (payload
    //           diretto degli eventi wasm_runtime.js). Frazione clampata.
    //   label    sovrascrive l'etichetta calcolata (solo per quest'update).
    function update(phase, progress, label) {
        phase = String(phase || 'download');
        if (phase === 'ready') { done(); return; }

        // normalizza il progresso
        let fraction = null;
        let loaded = null;
        let total = null;
        if (typeof progress === 'number' && Number.isFinite(progress)) {
            fraction = progress;
        } else if (progress && typeof progress === 'object') {
            if (typeof progress.fraction === 'number' && Number.isFinite(progress.fraction)) {
                fraction = progress.fraction;
            }
            if (Number.isFinite(progress.loaded)) loaded = progress.loaded;
            if (Number.isFinite(progress.total)) total = progress.total;
        }
        if (fraction == null && loaded != null && total != null && total > 0) {
            fraction = loaded / total;
        }
        if (phase === 'compile') {
            // la compilazione è una FASE: niente frazione NÉ byte del
            // download (non c'entrano con questa fase — nessuna % finta)
            fraction = null;
            loaded = null;
            total = null;
        }
        if (fraction != null) fraction = Math.max(0, Math.min(1, fraction));
        const determinate = fraction != null;

        // stato visivo: riappare se era nascosta/done/errore
        if (fadeTimer) { clearTimeout(fadeTimer); fadeTimer = null; }
        root.classList.remove('lb-hidden', 'lb-error', 'lb-done');
        root.style.visibility = '';
        root.style.pointerEvents = '';
        root.classList.toggle('lb-indeterminate', !determinate);
        // fuori dallo stato error la riga retry sparisce (regola CSS:
        // visibile solo con .lb-error E .lb-retryable)
        root.classList.toggle('lb-retryable', false);

        // fill: width inline in determinato; in indeterminato la prende il CSS (40%)
        fill.style.width = determinate ? (fraction * 100) + '%' : '';

        // etichetta: override esplicito > composta da fase/percentuale/byte
        const pct = determinate ? Math.floor(fraction * 100 + 1e-9) : null;
        const bytes = bytesPart(loaded, total);
        const text = label != null ? String(label)
            : composeLabel(labels[phase] != null ? labels[phase] : phase, pct, bytes);
        labelEl.textContent = text;

        // ARIA: % (intera, floor come l'etichetta visibile) + byte nel
        // valuetext; indeterminato -> nessun valuenow.
        if (determinate) setAria(pct, composeLabel(null, pct, bytes) || null);
        else setAria(null, text);

        state.phase = phase;
        state.fraction = fraction;
        state.label = text;
        state.hidden = false;
        state.done = false;
        state.error = null;
    }

    // error(message, retryHandler?) — stato errore: label/fill/pannello rossi,
    // messaggio + pulsante 'Riprova' (solo se c'è un handler: quello passato
    // qui, altrimenti opts.onRetry; senza nessuno dei due NESSUN bottone).
    // Riappare anche dopo un done() (errore tardivo).
    function error(message, retryHandler) {
        retryFn = typeof retryHandler === 'function' ? retryHandler : baseRetryFn;
        if (fadeTimer) { clearTimeout(fadeTimer); fadeTimer = null; }
        state.phase = 'error';
        state.error = String(message != null ? message : 'errore sconosciuto');
        state.fraction = null;
        state.done = false;
        state.hidden = false;

        root.classList.remove('lb-hidden', 'lb-indeterminate', 'lb-done');
        root.style.visibility = '';
        root.style.pointerEvents = '';
        root.classList.add('lb-error');
        root.classList.toggle('lb-retryable', typeof retryFn === 'function');

        fill.style.width = '100%'; // barra rossa piena: stato chiaro
        labelEl.textContent = labels.errorPrefix + state.error;
        state.label = labelEl.textContent;
        setAria(null, state.label); // niente valuenow: la % non c'entra più
    }

    // Retry click: prima la UI passa a 'Riprovo…' indeterminato, POI invoca
    // l'handler (che tipicamente rilancia il download: la sua prima update
    // sovrascrive lo stato retrying).
    retryBtn.addEventListener('click', () => {
        const fn = retryFn;
        update('retrying', null, null);
        if (fn) { try { fn(); } catch (e) { /* il retry non deve rompere la UI */ } }
    });

    // done() — app pronta: fill al 100%, aria completata, fade-out di
    // opacità e POI visibility:hidden. Il box RESTA nel layout: nessun
    // layout shift. Idempotente.
    function done() {
        if (state.done) return;
        state.phase = 'ready';
        state.done = true;
        state.error = null;
        root.classList.remove('lb-hidden', 'lb-indeterminate', 'lb-error', 'lb-retryable');
        root.classList.add('lb-done');
        root.style.visibility = '';
        root.style.pointerEvents = '';
        fill.style.width = '100%';
        labelEl.textContent = labels.ready;
        state.label = labels.ready;
        setAria(100, labels.ready);
        if (fadeTimer) { clearTimeout(fadeTimer); fadeTimer = null; }
        fadeTimer = setTimeout(() => {
            root.style.visibility = 'hidden';
            root.style.pointerEvents = 'none'; // mai click fantasma dopo il fade
        }, fadeMs + 60);
    }

    // show() — rivela la barra (la prima update() lo fa da sola).
    function show() {
        root.classList.remove('lb-hidden');
        root.style.visibility = '';
        root.style.pointerEvents = '';
        state.hidden = false;
    }

    // reset() — torna allo stato iniziale (nascosta, fill 0, aria pulita).
    // Utile tra i cicli demo o prima di un nuovo tentativo gestito fuori.
    function reset() {
        if (fadeTimer) { clearTimeout(fadeTimer); fadeTimer = null; }
        root.className = 'lb-root lb-hidden';
        root.style.visibility = '';
        root.style.pointerEvents = '';
        fill.style.width = '';
        labelEl.textContent = labels.download;
        setAria(null, null);
        state.phase = 'idle';
        state.fraction = null;
        state.label = labels.download;
        state.hidden = true;
        state.done = false;
        state.error = null;
    }

    // setLabel(text) — etichetta manuale senza toccare lo stato.
    function setLabel(text) {
        labelEl.textContent = String(text);
        state.label = labelEl.textContent;
    }

    // destroy() — rimuove il DOM dell'istanza (lo <style> è condiviso).
    function destroy() {
        if (fadeTimer) { clearTimeout(fadeTimer); fadeTimer = null; }
        root.remove();
    }

    return {
        update, error, done, show, reset, setLabel, destroy,
        get state() { return Object.assign({}, state); },
        refs: { root, panel, label: labelEl, track, fill, retryRow, retryBtn },
    };
}
