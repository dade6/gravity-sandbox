// wasm_runtime.js — Eventi del ciclo di vita del caricamento WASM.
//
// RENDE OSSERVABILE la fase di compilazione/istanziazione del WASM:
// WebAssembly.compile/instantiate NON offre progressi nativi, quindi usa
// una strategia a FASI — ogni fase del caricamento emette i propri eventi
// e la UI tratta la compilazione come INDETERMINATA (fase, non % finta).
//
// EVENTI (vedi LOADER_EVENTS, elencati in ordine di emissione):
//   'download-start'    {loaded, total, fraction, ts}   — headers ricevuti:
//                          fraction=0 (determinato) o null (indeterminato)
//   'download-progress' {loaded, total, fraction, ts}   — SOLO determinato:
//                          fraction crescente in (0,1], throttled dal loader
//   'download-end'      {loaded, total, byteLength, ts} — body completo
//   'compile-start'     {byteLength, streaming, ts}     — PRIMA della called
//                          REALE a WebAssembly.compile / compileStreaming
//   'compile-end'       {module, durationMs, byteLength, ts} — compilazione
//                          REALE terminata: module è il WebAssembly.Module
//   'ready'             {wasm, module, timings, ts}     — init() del glue ha
//                          risolto: l'app è avviata. timings =
//                          {downloadMs, compileMs, initMs}
//   'error'             {stage, message, detail, ts}    — emesso UNA sola
//                          volta per fallimento; stage = 'download' |
//                          'compile' | 'init'; detail = errore originale.
//
// USO (pattern consigliato, col bus):
//   import init from './gravity_sandbox_pXX.js';
//   import { createLoaderEvents, loadWasmLifecycle } from './wasm_runtime.js';
//   const events = createLoaderEvents();
//   events.on('download-progress', (p) => barSet(p.fraction)); // 0..1
//   events.on('compile-start',     () => barIndeterminate());  // fase, non %
//   events.on('ready',             () => barDone());
//   events.on('error',             (e) => showError(e.stage, e.message));
//   try {
//       await loadWasmLifecycle(WASM_URL, { events, sizeUrl, init });
//   } catch (err) {
//       // IL MODULO NON RITENTA: il retry resta al chiamante, es.
//       // initWithRetry() nel bootstrap (pattern Tailscale/DERP esistente).
//   }
//
// USO minimale (callback unica al posto del bus):
//   await loadWasmLifecycle(WASM_URL, {
//       init,
//       events: (name, payload) => console.log(name, payload),
//   });
//
// COMPOSIZIONE: loadWasmLifecycle = downloadWasm (wasm_download.js:
// progresso REALE dei byte, gzip-aware) → compileWasm (compilazione reale
// con eventi PRIMA/DOPO la chiamata) → initWasmApp (init del glue). Ogni
// pezzo è usabile da solo: vedi compileWasm()/initWasmApp() qui sotto.
//
// GLUE (wasm-bindgen): il WebAssembly.Module compilato qui viene passato
// a init({module_or_path: module}) — il glue NON ricompila né rifetcha:
// __wbg_load usa WebAssembly.instantiate(module, imports) (il pattern
// Safari/CORS resta preservato: nessun fetch interno). Se il body non è
// streamabile (browser senza r.body), downloadWasm restituisce la Response
// e la compilazione usa WebAssembly.compileStreaming(response).
//
// ANNULLAMENTO: opts.signal (AbortSignal) è controllato ai confini di
// fase; l'abort NON emette 'error' (è una cancellazione volontaria, non
// un fallimento di caricamento) e la Promise rigetta con AbortError.
//
// Il modulo non ha dipendenze oltre wasm_download.js; funziona su browser
// e Node 18+. Tutte le callback sono ISOLATE (try/catch): un errore della
// UI non può rompere il caricamento.

import { downloadWasm } from './wasm_download.js';

// I 7 eventi del ciclo di vita, in ordine di emissione (contratto pubblico
// stabile per la UI e per i test: webtests/run_compile_tests.mjs).
export const LOADER_EVENTS = [
    'download-start', 'download-progress', 'download-end',
    'compile-start', 'compile-end', 'ready', 'error',
];

// ---- helpers ----------------------------------------------------------

function nowMs() {
    return (typeof performance !== 'undefined' && performance.now)
        ? performance.now()
        : Date.now();
}

// Isolamento: una callback che lancia non deve rompere il caricamento.
function safeCall(fn, ...args) {
    try { fn(...args); } catch (e) {
        try { console.error('[wasm_runtime] callback isolata ha lanciato:', e); } catch (e2) { /* console assente */ }
    }
}

// Normalizza l'argomento `events`: accetta il bus di createLoaderEvents(),
// un qualunque oggetto {emit(name, payload)} o una funzione
// (name, payload) => void. null/undefined = nessun evento (uso
// programmatico silenzioso).
function normalizeEvents(events) {
    if (events == null) return null;
    if (typeof events === 'function') {
        return { emit: (name, payload) => safeCall(events, name, payload) };
    }
    if (typeof events.emit === 'function') return events;
    throw new TypeError(
        "wasm_runtime: 'events' deve essere il bus di createLoaderEvents(), " +
        'un oggetto {emit(name,payload)} o una funzione (name,payload)=>void');
}

function emit(events, name, payload) {
    if (!events) return;
    safeCall(() => events.emit(name, Object.assign({ ts: nowMs() }, payload)));
}

function abortErr() {
    try { return new DOMException('caricamento WASM annullato (AbortSignal)', 'AbortError'); }
    catch (e) {
        const err = new Error('caricamento WASM annullato (AbortSignal)');
        err.name = 'AbortError';
        return err;
    }
}

// Riporta il fallimento UNA volta sola: se un layer interno (compileWasm /
// initWasmApp usati da soli) ha già emesso 'error' per lo stesso errore, il
// marchio __loaderReported evita la duplicazione quando loadWasmLifecycle
// lo rilancia a sua volta.
function reportError(events, stage, err) {
    if (!events) return;
    if (err && typeof err === 'object' && err.__loaderReported) return;
    try { if (err && typeof err === 'object') err.__loaderReported = true; } catch (e) { /* errore frozen */ }
    emit(events, 'error', {
        stage,
        message: (err && err.message) ? String(err.message) : String(err),
        detail: err || null,
    });
}

// ---- EventBus minimale ------------------------------------------------

// createLoaderEvents() -> bus {on, off, emit, listenerCount}.
//   on(name, cb)      sottoscrive; RITORNA la funzione di unsubscribe.
//   off(name, cb)     rimuove la sottoscrizione.
//   emit(name, payload) -> n. callback chiamate. Le callback sono isolate:
//                      una che lancia non blocca le altre né il caricamento.
//                      snapshot: on/off DENTRO una callback non rompono emit.
export function createLoaderEvents() {
    const subs = new Map(); // name -> Set<cb>
    const bus = {
        on(name, cb) {
            if (typeof cb !== 'function') {
                throw new TypeError("createLoaderEvents().on('" + name + "'): cb non è una funzione");
            }
            let set = subs.get(name);
            if (!set) { set = new Set(); subs.set(name, set); }
            set.add(cb);
            return () => bus.off(name, cb); // unsubscribe
        },
        off(name, cb) {
            const set = subs.get(name);
            if (set) { set.delete(cb); if (set.size === 0) subs.delete(name); }
        },
        emit(name, payload) {
            const set = subs.get(name);
            if (!set || set.size === 0) return 0;
            let delivered = 0;
            for (const cb of Array.from(set)) {
                safeCall(cb, payload, name);
                delivered++;
            }
            return delivered;
        },
        listenerCount(name) {
            const set = subs.get(name);
            return set ? set.size : 0;
        },
    };
    return bus;
}

// ---- compilazione con eventi (AC1) -------------------------------------

// compileWasm(source, opts) -> Promise<WebAssembly.Module>
// Compila il wasm emettendo 'compile-start' PRIMA della chiamata reale e
// 'compile-end' DOPO: gli eventi circondano la compilazione REALE.
//   source: Uint8Array (→ WebAssembly.compile) | Response (→ compileStreaming)
//   opts.events:     bus/callback (opzionale)
//   opts.compileImpl: compile sostituibile PER I TEST (solo sorgente bytes;
//                    iniettarlo disattiva la via compileStreaming)
//   opts.signal:     annullamento ai confini di fase
// Su fallimento emette 'error' {stage:'compile', message, detail} e RILANCIA
// l'errore originale (marcato __loaderReported: nessun duplicato a monte).
// La durata reale (durationMs, dentro la chiamata) viaggia con compile-end.
export async function compileWasm(source, opts = {}) {
    const events = normalizeEvents(opts.events);
    const isResponse = typeof Response === 'function' && source instanceof Response;
    const byteLength = isResponse ? null : (source ? (source.byteLength || 0) : 0);

    if (opts.signal && opts.signal.aborted) throw abortErr();
    if (!isResponse && typeof WebAssembly.compile !== 'function') {
        throw new TypeError('wasm_runtime: WebAssembly.compile non disponibile in questo ambiente');
    }

    emit(events, 'compile-start', { byteLength, streaming: isResponse });
    const t0 = nowMs();
    let module;
    try {
        if (isResponse) {
            if (!opts.compileImpl && typeof WebAssembly.compileStreaming === 'function') {
                try {
                    module = await WebAssembly.compileStreaming(source);
                } catch (streamErr) {
                    // MIME non application/wasm o body non riavvolgibile:
                    // fallback esplicito su arrayBuffer+compile (stesso
                    // pattern del glue wasm-bindgen). Se il body è già stato
                    // consumato a metà stream, questo rigetta e il fallimento
                    // viene riportato sotto — comportamento onesto.
                    const bytes = new Uint8Array(await source.arrayBuffer());
                    module = await (opts.compileImpl || WebAssembly.compile.bind(WebAssembly))(bytes);
                }
            } else {
                const bytes = new Uint8Array(await source.arrayBuffer());
                module = await (opts.compileImpl || WebAssembly.compile.bind(WebAssembly))(bytes);
            }
        } else {
            const compileImpl = opts.compileImpl || WebAssembly.compile.bind(WebAssembly);
            module = await compileImpl(source);
        }
    } catch (err) {
        reportError(events, 'compile', err);
        throw err;
    }
    emit(events, 'compile-end', { module, durationMs: nowMs() - t0, byteLength });
    return module;
}

// ---- istanziazione/init col glue ---------------------------------------

// initWasmApp(module, initFn, opts) -> Promise<wasm>
// Esegue initFn({module_or_path: module}) — il glue wasm-bindgen istanzia
// il Module SENZA fetch né ricompilazione — ed emette 'ready' quando init
// risolve (app avviata).
//   module: DEVE essere un WebAssembly.Module (da compileWasm)
//   initFn: funzione init del glue (es. import init from '..._pXX.js')
//   opts.timings: riportato verbatim nell'evento 'ready' (usato da
//                 loadWasmLifecycle per aggiungere downloadMs/compileMs)
// Su fallimento emette 'error' {stage:'init'} e rilancia.
export async function initWasmApp(module, initFn, opts = {}) {
    const events = normalizeEvents(opts.events);
    if (typeof initFn !== 'function') {
        throw new TypeError('wasm_runtime: initWasmApp richiede la funzione init() del glue');
    }
    if (typeof WebAssembly === 'undefined' || !(module instanceof WebAssembly.Module)) {
        throw new TypeError('wasm_runtime: initWasmApp richiede un WebAssembly.Module (da compileWasm)');
    }
    if (opts.signal && opts.signal.aborted) throw abortErr();

    const t0 = nowMs();
    let wasm;
    try {
        wasm = await initFn({ module_or_path: module });
    } catch (err) {
        reportError(events, 'init', err);
        throw err;
    }
    const timings = Object.assign({}, opts.timings || {}, { initMs: nowMs() - t0 });
    emit(events, 'ready', { wasm, module, timings });
    return wasm;
}

// ---- pipeline completa --------------------------------------------------

// loadWasmLifecycle(url, opts) -> Promise<wasm>
// Scarica (progresso reale), compila (eventi attorno alla compilazione
// REALE) e inizializza (glue) il wasm, emettendo l'intero ciclo di vita.
//   opts.init:        (REQUIRED) funzione init del glue
//   opts.events:      bus di createLoaderEvents() | (name,payload)=>void | {emit}
//   opts.sizeUrl:     sidecar con la taglia decompressa (vedi wasm_download.js)
//   opts.throttleMs:  throttling dei download-progress (default 100ms)
//   opts.fetchImpl:   fetch iniettabile (test)
//   opts.compileImpl: compile sostituibile (test; solo sorgente bytes)
//   opts.signal:      AbortSignal ai confini di fase (abort ≠ 'error')
// Risolve col valore ritornato da init (le export wasm). Su fallimento:
// UN evento 'error' {stage: download|compile|init} + rigetto con l'errore
// originale. NON ritenta: il retry resta al chiamante (initWithRetry).
export async function loadWasmLifecycle(url, opts = {}) {
    const events = normalizeEvents(opts.events);
    if (typeof opts.init !== 'function') {
        throw new TypeError("wasm_runtime: loadWasmLifecycle richiede opts.init (la funzione init() del glue)");
    }
    if (opts.signal && opts.signal.aborted) throw abortErr();

    // FASE 1 — download con progresso reale (wasm_download.js): le fasi
    // interne start/progress/end sono rimappate 1:1 su download-start /
    // download-progress / download-end. fraction null = indeterminato.
    const tDown0 = nowMs();
    let dl;
    try {
        dl = await downloadWasm(url, {
            sizeUrl: opts.sizeUrl,
            throttleMs: opts.throttleMs,
            fetchImpl: opts.fetchImpl,
            signal: opts.signal,
            onProgress: (p) => {
                if (p.phase === 'start') {
                    emit(events, 'download-start', { loaded: 0, total: p.total, fraction: p.fraction });
                } else if (p.phase === 'progress') {
                    emit(events, 'download-progress', { loaded: p.loaded, total: p.total, fraction: p.fraction });
                } else if (p.phase === 'end') {
                    emit(events, 'download-end', { loaded: p.loaded, total: p.total, byteLength: p.loaded, fraction: p.fraction });
                }
            },
        });
    } catch (err) {
        reportError(events, 'download', err);
        throw err;
    }
    const downloadMs = nowMs() - tDown0;

    // FASE 2 — compilazione REALE con eventi attorno. bytes è il percorso
    // normale; se il body non era streamabile dl.bytes è null e si compila
    // dalla Response (compileStreaming).
    const tComp0 = nowMs();
    let module;
    try {
        module = await compileWasm(dl.bytes || dl.response, {
            events,
            compileImpl: opts.compileImpl,
            signal: opts.signal,
        });
    } catch (err) {
        reportError(events, 'compile', err); // dedup: compileWasm ha già emesso
        throw err;
    }
    const compileMs = nowMs() - tComp0;

    // FASE 3 — istanziazione/init del glue → 'ready' (app avviata).
    return initWasmApp(module, opts.init, {
        events,
        signal: opts.signal,
        timings: { downloadMs, compileMs },
    });
}
