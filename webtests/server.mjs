// webtests/server.mjs — HTTP server con fixture WASM per i test di
// wasm_download.js. Genera un modulo WASM REALE e valido (export f()->i32
// 42 + custom section da 1.5MB di payload deterministico) e lo serve in
// varianti che coprono i criteri di accettazione della card:
//
//   /fixture.wasm          CL presente, non compresso (determinato)
//   /fixture-chunked.wasm  Transfer-Encoding chunked, NO Content-Length
//                          (indeterminato, nessun errore)
//   /fixture-gz.wasm       Content-Encoding: gzip + CL compressa +
//                          X-Wasm-Decompressed-Length (determinato:
//                          il totale è la taglia DECOMPRESSA)
//   /fixture-gz-nohdr.wasm gzip + CL compressa SENZA X-header: il CL
//                          compresso NON è un totale valido -> il modulo
//                          deve restare indeterminato (trappola gzip)
//   /fixture.size          sidecar: taglia decompressa come testo
//   /wasm_download.js      il modulo sotto test (dal repo root)
//   /test-page.html        pagina host dei test
//
// I body .wasm sono scritti a chunk (48KB ogni 45ms) così il download
// dura ~1.4s e la callback di progresso scatta PIÙ volte (criterio 1).
import http from 'node:http';
import { readFileSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const PORT = parseInt(process.argv[2] || '8181', 10);
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

// ---- fixture WASM (valida: header + sezioni type/func/export/code) ----
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
function buildFixture(payloadSize) {
    const header = Uint8Array.from([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);
    const typeSec = Uint8Array.from([0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f]); // () -> i32
    const funcSec = Uint8Array.from([0x03, 0x02, 0x01, 0x00]);
    const exportSec = Uint8Array.from([0x07, 0x05, 0x01, 0x01, 0x66, 0x00, 0x00]); // export "f"
    const codeSec = Uint8Array.from([0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b]); // i32.const 42
    // custom section "pad" con payload deterministico (LCG) — mantiene il
    // modulo VALIDO qualunque sia il contenuto (custom = sezione ignota)
    const payload = new Uint8Array(payloadSize);
    let x = 123456789 >>> 0;
    for (let i = 0; i < payloadSize; i++) {
        x = (Math.imul(x, 1664525) + 1013904223) >>> 0;
        payload[i] = x >>> 24;
    }
    const name = Uint8Array.from([0x03, 0x70, 0x61, 0x64]); // len 3, "pad"
    const secBody = concat(name, payload);
    const customSec = concat(Uint8Array.from([0x00]), leb128(secBody.length), secBody);
    return concat(header, typeSec, funcSec, exportSec, codeSec, customSec);
}

const FIXTURE = buildFixture(1.5 * 1024 * 1024);
const FIXTURE_GZ = gzipSync(FIXTURE);
// Sanity: la fixture DEVE essere un wasm valido e istanziabile (Node 22
// ha WebAssembly globale). Fallire qui = fixture rotta, non test fallito.
if (!WebAssembly.validate(FIXTURE)) throw new Error('fixture WASM non valida');
{
    const { instance } = await WebAssembly.instantiate(FIXTURE, {});
    if (instance.exports.f() !== 42) throw new Error('fixture export f() != 42');
}
console.log(`[webtests] fixture: ${FIXTURE.length} bytes (gz ${FIXTURE_GZ.length}), wasm valido, f()=42`);

const CHUNK = 48 * 1024;
const CHUNK_DELAY_MS = 45; // ~32 chunk -> download ~1.4s -> molti progress

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
    } else if (p === '/fixture-chunked.wasm') {
        // NESSUN Content-Length -> Node usa Transfer-Encoding: chunked
        pacedWrite(res, FIXTURE, { ...baseHeaders });
    } else if (p === '/fixture-gz.wasm') {
        pacedWrite(res, FIXTURE_GZ, {
            ...baseHeaders,
            'Content-Length': String(FIXTURE_GZ.length),
            'Content-Encoding': 'gzip',
            'Vary': 'Accept-Encoding',
            'X-Wasm-Decompressed-Length': String(FIXTURE.length),
        });
    } else if (p === '/fixture-gz-nohdr.wasm') {
        // gzip senza X-Wasm-Decompressed-Length: CL è la taglia COMPRESSA,
        // inutilizzabile come totale -> il modulo deve andare indeterminato
        pacedWrite(res, FIXTURE_GZ, {
            ...baseHeaders,
            'Content-Length': String(FIXTURE_GZ.length),
            'Content-Encoding': 'gzip',
            'Vary': 'Accept-Encoding',
        });
    } else if (p === '/fixture.size') {
        res.writeHead(200, { 'Content-Type': 'text/plain', 'Cache-Control': 'no-store', 'Access-Control-Allow-Origin': '*' });
        res.end(String(FIXTURE.length));
    } else if (p === '/wasm_download.js') {
        const js = readFileSync(path.join(REPO, 'wasm_download.js'));
        res.writeHead(200, { 'Content-Type': 'application/javascript', 'Cache-Control': 'no-store', 'Access-Control-Allow-Origin': '*' });
        res.end(js);
    } else if (p === '/test-page.html') {
        const html = readFileSync(path.join(REPO, 'webtests', 'test-page.html'));
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
    console.log(`[webtests] fixture server su http://127.0.0.1:${PORT}`);
});
