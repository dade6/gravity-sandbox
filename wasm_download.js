// wasm_download.js — Download streaming del .wasm con progresso REALE.
//
// Sostituisce il fetch semplice del .wasm con un fetch instrumentato:
// legge il body come ReadableStream, accumula i byte ricevuti e confronta
// con la taglia totale per emettere callback di progresso
// {loaded, total, fraction} con throttling (default 100ms = max ~10
// aggiornamenti/sec). Se la taglia totale non è conoscibile (risposta
// chunked senza Content-Length, o gzip senza X-Wasm-Decompressed-Length
// né sidecar), passa in modalità INDETERMINATA: emette SOLO start/end,
// mai percentuali inventate.
//
// CONTRATTO A VALLE (invariato rispetto al fetch semplice): la Promise
// risolve con { bytes, response, byteLength, total, determinate } dove
// `bytes` è lo Uint8Array completo del .wasm — un BufferSource che
// wasm-bindgen compila direttamente (init({ module_or_path: bytes })),
// senza alcun fetch interno del glue (pattern Safari/CORS preservato).
// Se il body non è streamabile (browser senza r.body) `bytes` è null e
// `response` è la Response originale: passare quella a init(), come
// faceva il codice precedente.
//
// GZIP (serve_wasm.py serve il .gz con Content-Encoding: gzip): il
// reader emette byte DECOMPRESSI mentre Content-Length è la taglia
// COMPRESSA — usare CL come totale sforerebbe oltre 100%. Il totale
// DECOMPRESSO arriva (in ordine di priorità) da:
//   1. header X-Wasm-Decompressed-Length (0 round-trip extra)
//   2. file sidecar sizeUrl (es. gravity_sandbox_bg.wasm.size, pochi
//      byte, scritto da build_wasm.sh: contiene la taglia decompressa)
// Senza nessuno dei due: indeterminata.
// Con risposta NON gzip: Content-Length è la taglia esatta (il reader
// emette i byte grezzi); il sidecar è fallback se CL manca (chunked).
//
// USO:
//   import { downloadWasm } from './wasm_download.js';
//   const d = await downloadWasm(WASM_URL, {
//     sizeUrl: WASM_SIZE_URL,             // sidecar opzionale
//     onProgress: (p) => updateBar(p),   // vedi eventi qui sotto
//   });
//   await init({ module_or_path: d.bytes || d.response });
//
// EVENTI onProgress(p): p = { phase, loaded, total, fraction, done }
//   phase 'start' — 1 volta, subito dopo gli header: total/fraction
//     noti se la taglia è affidabile, altrimenti null (indeterminata).
//   phase 'progress' — SOLO in modo determinato, con throttle ≤
//     throttleMs; frazione crescente in (0,1). La PRIMA emissione è
//     immediata (nessun ritardo artificiale all'inizio).
//   phase 'end' — sempre, 1 volta, MAI throttled via: loaded = byte
//     ricevuti, fraction = 1 (determinato) o null (indeterminato),
//     done = true.
//   In modalità indeterminata: SOLO start + end (nessun 'progress').
//
// Il modulo non ha dipendenze e funziona su browser e Node 18+
// (usa solo fetch/Response/ReadableStream standard). fetchImpl è
// iniettabile per i test.

export const DEFAULT_THROTTLE_MS = 100; // max ~10 aggiornamenti/sec

// ---- helpers -----------------------------------------------------------

// Parse robusto di un intero positivo (header o sidecar). null se assente
// o non valido: MAI usare NaN/0 come totale.
function parsePositiveInt(v) {
    if (v == null) return null;
    const n = parseInt(String(v).trim(), 10);
    return Number.isFinite(n) && n > 0 ? n : null;
}

// Pre-fetch best-effort del sidecar con la taglia DECOMPRESSA. Pochi
// byte; un fallimento NON è un errore del download: restituisce null e
// (se manca anche l'header) il progresso resta indeterminato.
async function fetchSidecarSize(sizeUrl, fetchImpl) {
    if (!sizeUrl) return null;
    try {
        const r = await fetchImpl(sizeUrl, { cache: 'no-store' });
        if (!r.ok) return null;
        return parsePositiveInt(await r.text());
    } catch (e) {
        return null;
    }
}

// ---- modulo -----------------------------------------------------------

export async function downloadWasm(url, opts = {}) {
    const fetchImpl = opts.fetchImpl || globalThis.fetch.bind(globalThis);
    const throttleMs = opts.throttleMs != null ? opts.throttleMs : DEFAULT_THROTTLE_MS;
    const onProgress = typeof opts.onProgress === 'function' ? opts.onProgress : null;
    const sizeUrl = opts.sizeUrl || null;

    const emit = (p) => { if (onProgress) { try { onProgress(p); } catch (e) { /* la UI non deve rompere il download */ } } };

    // Sidecar PRIMA del fetch principale (pochi byte, no-store come il
    // resto del flusso di caricamento).
    const sideSize = await fetchSidecarSize(sizeUrl, fetchImpl);

    const reqInit = Object.assign({ cache: 'no-store' }, opts.request || {});
    if (opts.signal) reqInit.signal = opts.signal;

    const r = await fetchImpl(url, reqInit);
    if (!r.ok) throw new Error('wasm fetch HTTP ' + r.status);

    // ---- totale affidabile per la % ------------------------------------
    // gzip  -> il reader emette byte DECOMPRESSI: il totale vero è la
    //          taglia decompressa (header X-... o sidecar), NON il
    //          Content-Length (che è la taglia compressa).
    // identico -> il reader emette i byte grezzi: Content-Length è il
    //          totale esatto; sidecar solo se CL manca (chunked).
    const enc = (r.headers.get('Content-Encoding') || '').toLowerCase();
    const gzip = enc.indexOf('gzip') >= 0 || enc.indexOf('deflate') >= 0;
    let total = null;
    if (gzip) {
        const hDec = parsePositiveInt(r.headers.get('X-Wasm-Decompressed-Length'));
        total = hDec != null ? hDec : sideSize;
    } else {
        const cl = parsePositiveInt(r.headers.get('Content-Length'));
        total = cl != null ? cl : sideSize;
    }
    const determinate = total != null;

    // start: fraction 0 se determinato, null altrimenti.
    emit({ phase: 'start', loaded: 0, total, fraction: determinate ? 0 : null, done: false });

    // ---- body non streamabile: Response diretta al glue (come prima) ----
    if (!r.body) {
        emit({ phase: 'end', loaded: 0, total, fraction: null, done: true });
        return { bytes: null, response: r, byteLength: 0, total, determinate: false };
    }

    // ---- lettura a chunk con accumulo e throttle ------------------------
    const reader = r.body.getReader();
    const chunks = [];
    let received = 0;
    let lastEmit = 0;      // timestamp dell'ultimo 'progress' emesso
    let warnedClamp = false;

    const doRead = () => reader.read().then(({ done, value }) => {
        if (done) {
            // concatena i chunk in UNO Uint8Array (allocato una volta sola)
            let bytes;
            if (chunks.length === 1 && chunks[0].byteLength === received) {
                bytes = chunks[0]; // zero-copy: chunk unico già completo
            } else {
                bytes = new Uint8Array(received);
                let off = 0;
                for (const c of chunks) { bytes.set(c, off); off += c.byteLength; }
            }
            // progresso FINALE con fraction=1 (fuori throttle): la frazione
            // cresce davvero da 0 a 1 e la barra arriva piena al 100%
            // prima della fase successiva. Dedupe se l'ultimo già era 1
            // (es. clamp per header/sidecar fuori sincrono).
            if (determinate && received > 0 && lastEmit !== 1) {
                lastEmit = 1;
                emit({ phase: 'progress', loaded: received, total, fraction: 1, done: false });
            }
            // end: sempre emesso, mai throttled.
            emit({
                phase: 'end',
                loaded: received,
                total: determinate ? total : null,
                fraction: determinate ? 1 : null,
                done: true,
            });
            return { bytes, response: null, byteLength: received, total, determinate };
        }
        chunks.push(value);
        received += value.byteLength;

        // 'progress' SOLO in modo determinato. Throttle a tempo con
        // eccezioni: la prima emissione è immediata (la barra parte
        // subito) e l'evento 'end' separato non è mai throttled.
        if (determinate && received > 0) {
            const now = Date.now();
            if (lastEmit === 0 || now - lastEmit >= throttleMs) {
                lastEmit = now;
                let frac = received / total;
                if (frac > 1) {
                    // header/sidecar fuori sincrono coi byte reali: clamp,
                    // mai superare 100%; il download resta valido.
                    frac = 1;
                    if (!warnedClamp) {
                        warnedClamp = true;
                        try { console.warn('[wasm_download] totale (' + total + ') < byte ricevuti (' + received + '): % clampata a 100%'); } catch (e) {}
                    }
                }
                emit({ phase: 'progress', loaded: received, total, fraction: frac, done: false });
            }
        }
        return doRead();
    });

    // Su errore a metà stream: rilascia il reader e propaga (il chiamante
    // gestisce il retry, es. initWithRetry nel bootstrap).
    try {
        return await doRead();
    } catch (e) {
        try { reader.cancel(); } catch (e2) { /* già chiuso/rotto */ }
        throw e;
    }
}
