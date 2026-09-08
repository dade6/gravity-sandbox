// webtests/compile_server.mjs — fixture server DEDICATO ai test di
// wasm_runtime.js (card t_3c91ee24). Autocontenuto: NON tocca server.mjs
// (di proprietà della card sibling t_d212617b) per evitare conflitti.
//
//   /fixture.wasm      wasm valido ~1.5MB (CL presente, paced 48KB/45ms)
//                      — per la pipeline completa happy-path
//   /fixture-big.wasm  wasm valido con 200.000 funzioni (~1MB): la
//                      compilazione REALE dura abbastanza da rendere
//                      durationMs misurabile (>0 in modo robusto)
//   /fixture-bad.wasm  header wasm valido + sezione type TRONCATA:
//                      WebAssembly.compile lancia CompileError REALE
//                      (criterio AC2: errore di compilazione → 'error')
//   /fixture.size      sidecar: taglia decompressa di fixture.wasm
//   /wasm_runtime.js  /wasm_download.js   moduli sotto test (repo root)
//   /compile-test-page.html               pagina host dei test
//   /shutdown POST                         spegne il server
import http from 'node:http';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const PORT = parseInt(process.argv[2] || '8183', 10);
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

// ---- fixture WASM -------------------------------------------------------
function concat(...arrs) {
    const n = arrs.reduce((a, b) => a + b.length, 0);
    const out = new Uint8Array(n);
    let o = 0;
    for (const a of arrs) { out.set(a, o); o += a.length; }
    return out;
}
function leb128(n) { // unsigned LEB128
    const out = [];
    do { let b = n & 0x7f; n = Math.floor(n / 128); if (n > 0) b |= 0x80; out.push(b); } while (n > 0);
    return Uint8Array.from(out);
}
const WASM_HEADER = Uint8Array.from([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);
const TYPE_SEC = Uint8Array.from([0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f]); // () -> i32
const FUNC_SEC_1 = Uint8Array.from([0x03, 0x02, 0x01, 0x00]);                  // 1 func di tipo 0
const EXPORT_SEC = Uint8Array.from([0x07, 0x05, 0x01, 0x01, 0x66, 0x00, 0x00]); // export "f" idx 0
const CODE_SEC_1 = Uint8Array.from([0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b]); // i32.const 42

// custom section "pad" col payload LCG deterministico (sezione ignota:
// il modulo resta valido qualunque sia il contenuto)
function padSection(payloadSize) {
    const payload = new Uint8Array(payloadSize);
    let x = 123456789 >>> 0;
    for (let i = 0; i < payloadSize; i++) {
        x = (Math.imul(x, 1664525) + 1013904223) >>> 0;
        payload[i] = x >>> 24;
    }
    const name = Uint8Array.from([0x03, 0x70, 0x61, 0x64]); // len 3, "pad"
    const secBody = concat(name, payload);
    return concat(Uint8Array.from([0x00]), leb128(secBody.length), secBody);
}

// 200.000 funzioni () -> i32, corpo `i32.const 42`: type/func/export/code
// sections piene. Export della funzione 0 come "f" per poter verificare
// l'istanziazione. La compilazione di ~1MB di codice reale su V8 dura
// un tempo misurabile (decine/centinaia di ms).
// Sezione = id (1 byte) + LEB128(taglia del CORPO) + corpo; le sezioni
// vanno in ORDINE canonico (type 1 < func 3 < export 7 < code 10).
function section(id, body) {
    return concat(Uint8Array.from([id]), leb128(body.length), body);
}
function buildBigFixture(nFuncs) {
    const typeSec = TYPE_SEC;
    // func section body: count + nFuncs * [typeidx 0]
    const funcSec = section(0x03, concat(leb128(nFuncs), new Uint8Array(nFuncs)));
    // code section body: count + nFuncs * [body_size=4, 0 locals,
    // i32.const 42, end]
    const oneBody = Uint8Array.from([0x04, 0x00, 0x41, 0x2a, 0x0b]);
    const bodies = new Uint8Array(nFuncs * 5);
    for (let i = 0; i < nFuncs; i++) bodies.set(oneBody, i * 5);
    const codeSec = section(0x0a, concat(leb128(nFuncs), bodies));
    // export section: 1 export "f" kind func idx 0
    const exportSec = EXPORT_SEC;
    return concat(WASM_HEADER, typeSec, funcSec, exportSec, codeSec);
}

const FIXTURE = concat(WASM_HEADER, TYPE_SEC, FUNC_SEC_1, EXPORT_SEC, CODE_SEC_1, padSection(1.5 * 1024 * 1024));
const FIXTURE_BIG = buildBigFixture(200000);
// header VALIDO + type section che dichiara 127 byte di corpo MAI forniti
// (payload troncato): compile() fallisce DOPO aver letto l'header — errore
// di compilazione reale, non di rete né di download.
const FIXTURE_BAD = concat(
    WASM_HEADER,
    Uint8Array.from([0x01, 0x7f]), // section id 1 (type), size 127... e basta
    new Uint8Array(8), // qualcosina in più: né valida né vuota
);

// sanity: le fixture valide DEVONO compilare e f()=42 (Node 18+ ha WebAssembly)
if (!WebAssembly.validate(FIXTURE)) throw new Error('fixture WASM non valida');
if (!WebAssembly.validate(FIXTURE_BIG)) throw new Error('fixture BIG non valida');
{
    const { instance } = await WebAssembly.instantiate(FIXTURE, {});
    if (instance.exports.f() !== 42) throw new Error('fixture export f() != 42');
}
{
    const { instance } = await WebAssembly.instantiate(FIXTURE_BIG, {});
    if (instance.exports.f() !== 42) throw new Error('fixture BIG export f() != 42');
}
if (WebAssembly.validate(FIXTURE_BAD)) throw new Error('fixture BAD non deve essere valida');
console.log(`[compile-server] fixture: small=${FIXTURE.length}B big=${FIXTURE_BIG.length}B bad=${FIXTURE_BAD.length}B`);

const CHUNK = 48 * 1024;
const CHUNK_DELAY_MS = 45;

function pacedWrite(res, body, headers) {
    res.writeHead(200, headers);
    let off = 0;
    let aborted = false;
    res.on('close', () => { aborted = true; });
    const send = () => {
        if (aborted) return;
        if (off >= body.length) { res.end(); return; }
        const end = Math.min(off + CHUNK, body.length);
        res.write(Buffer.from(body.subarray(off, end)));
        off = end;
        setTimeout(send, CHUNK_DELAY_MS);
    };
    send();
}
const baseHeaders = {
    'Content-Type': 'application/wasm',
    'Cache-Control': 'no-store',
    'Access-Control-Allow-Origin': '*',
};

const server = http.createServer((req, res) => {
    const p = req.url.split('?')[0];
    if (p === '/fixture.wasm') {
        pacedWrite(res, FIXTURE, { ...baseHeaders, 'Content-Length': String(FIXTURE.length) });
    } else if (p === '/fixture-big.wasm') {
        pacedWrite(res, FIXTURE_BIG, { ...baseHeaders, 'Content-Length': String(FIXTURE_BIG.length) });
    } else if (p === '/fixture-bad.wasm') {
        pacedWrite(res, FIXTURE_BAD, { ...baseHeaders, 'Content-Length': String(FIXTURE_BAD.length) });
    } else if (p === '/fixture.size') {
        res.writeHead(200, { 'Content-Type': 'text/plain', 'Cache-Control': 'no-store', 'Access-Control-Allow-Origin': '*' });
        res.end(String(FIXTURE.length));
    } else if (p === '/wasm_runtime.js' || p === '/wasm_download.js') {
        const js = readFileSync(path.join(REPO, p.slice(1)));
        res.writeHead(200, { 'Content-Type': 'application/javascript', 'Cache-Control': 'no-store', 'Access-Control-Allow-Origin': '*' });
        res.end(js);
    } else if (p === '/compile-test-page.html') {
        const html = readFileSync(path.join(REPO, 'webtests', 'compile-test-page.html'));
        res.writeHead(200, { 'Content-Type': 'text/html', 'Cache-Control': 'no-store' });
        res.end(html);
    } else if (p === '/shutdown' && req.method === 'POST') {
        res.writeHead(204); res.end();
        setTimeout(() => process.exit(0), 50);
    } else {
        res.writeHead(404, { 'Content-Type': 'text/plain' });
        res.end('not found: ' + p);
    }
});
server.listen(PORT, '127.0.0.1', () => {
    console.log(`[compile-server] fixture server su http://127.0.0.1:${PORT}`);
});
