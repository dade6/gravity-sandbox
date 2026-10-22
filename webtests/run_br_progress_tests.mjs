// Test del calcolo della % in wasm_download.js quando il server manda
// brotli (Content-Encoding: br). Il reader del browser emette byte
// DECOMPRESSI: il totale deve venire dal sidecar .size, non dal
// Content-Length (che e' la taglia compressa).
//
// Gira senza browser (fetch/Response/ReadableStream di Node 18+):
//   node webtests/run_br_progress_tests.mjs
import { downloadWasm } from '../wasm_download.js';

const DECOMPRESSED = 1000;
const COMPRESSED = 132;

function makeFetch(encoding) {
    return async (url) => {
        if (String(url).includes('.size')) {
            return new Response(String(DECOMPRESSED), { status: 200 });
        }
        const body = new ReadableStream({
            start(c) {
                c.enqueue(new Uint8Array(DECOMPRESSED / 2));
                c.enqueue(new Uint8Array(DECOMPRESSED / 2));
                c.close();
            },
        });
        const headers = { 'Content-Type': 'application/wasm', 'Content-Length': String(COMPRESSED) };
        if (encoding) headers['Content-Encoding'] = encoding;
        return new Response(body, { status: 200, headers });
    };
}

let failures = 0;
const check = (name, cond, detail) => {
    if (!cond) failures++;
    console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${detail ? '  [' + detail + ']' : ''}`);
};

// --- brotli ---
let events = [];
let res = await downloadWasm('http://x/gravity_sandbox_bg.wasm?v=1', {
    sizeUrl: 'http://x/gravity_sandbox_bg.wasm.size?v=1',
    fetchImpl: makeFetch('br'),
    throttleMs: 0,
    onProgress: (p) => events.push(p),
});
check('br: totale = taglia decompressa (dal sidecar)', res.total === DECOMPRESSED, String(res.total));
check('br: determinato', res.determinate === true);
check('br: byte letti', res.byteLength === DECOMPRESSED, String(res.byteLength));
check('br: ultimo evento fraction 1', events[events.length - 1].fraction === 1,
    String(events[events.length - 1].fraction));
check('br: nessuna frazione > 1', events.every((e) => e.fraction == null || e.fraction <= 1));
check('br: ci sono progressi intermedi',
    events.some((e) => e.phase === 'progress' && e.loaded < e.total));

// --- gzip (contratto preesistente, non deve regredire) ---
events = [];
res = await downloadWasm('http://x/w.wasm', {
    sizeUrl: 'http://x/w.wasm.size',
    fetchImpl: makeFetch('gzip'),
    throttleMs: 0,
    onProgress: (p) => events.push(p),
});
check('gzip: totale dal sidecar', res.total === DECOMPRESSED, String(res.total));

// --- identity: il totale viene dal Content-Length ---
events = [];
res = await downloadWasm('http://x/w.wasm', {
    sizeUrl: 'http://x/w.wasm.size',
    fetchImpl: async (url) => {
        if (String(url).includes('.size')) return new Response('999999');
        return new Response(new Uint8Array(DECOMPRESSED), {
            status: 200,
            headers: { 'Content-Length': String(DECOMPRESSED) },
        });
    },
    throttleMs: 0,
    onProgress: (p) => events.push(p),
});
check('identity: totale = Content-Length, non il sidecar', res.total === DECOMPRESSED, String(res.total));

console.log('');
console.log(failures === 0 ? 'OK — nessuna regressione' : `${failures} CONTROLLI FALLITI`);
process.exit(failures === 0 ? 0 : 1);
