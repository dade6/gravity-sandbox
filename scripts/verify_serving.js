#!/usr/bin/env node
// Verifica end-to-end di come il server consegna il wasm a un client tipo
// Safari: brotli/gzip negoziati, Content-Type, cache, integrita' byte per byte.
//
// Uso: node scripts/verify_serving.js [baseUrl] [versione]
//   node scripts/verify_serving.js http://127.0.0.1:8081 v0.14.153
//   NODE_TLS_REJECT_UNAUTHORIZED=0 node scripts/verify_serving.js https://127.0.0.1:9443 v0.14.153
//
// NB: usa http/https grezzi di Node, NON fetch(). fetch (undici) decomprime
// da solo Content-Encoding e toglierebbe il corpo compresso: il test
// finirebbe per decomprimere due volte. Qui vogliamo i byte ESATTI che il
// browser riceve, per confrontarli con il wasm su disco.
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const crypto = require('crypto');
const http = require('http');
const https = require('https');

const BASE = (process.argv[2] || 'http://127.0.0.1:8081').replace(/\/$/, '');
const VERSION = (process.argv[3] || 'v0.14.153').replace(/^v/, '');
const WASM = path.join(__dirname, '..', 'wasm-dist', 'gravity_sandbox_bg.wasm');

const sha = (buf) => crypto.createHash('sha256').update(buf).digest('hex');
let failures = 0;

function check(name, cond, detail) {
    if (!cond) failures++;
    console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${detail ? '  [' + detail + ']' : ''}`);
}

// Richiesta HTTP grezza: nessuna decompressione automatica, header intatti.
function rawGet(urlPath, headers = {}) {
    const u = new URL(BASE + urlPath);
    const lib = u.protocol === 'https:' ? https : http;
    const t0 = Date.now();
    return new Promise((resolve, reject) => {
        const req = lib.request(
            { hostname: u.hostname, port: u.port, path: u.pathname + u.search, method: 'GET', headers },
            (res) => {
                const chunks = [];
                res.on('data', (c) => chunks.push(c));
                res.on('end', () => resolve({
                    status: res.statusCode,
                    headers: res.headers,
                    body: Buffer.concat(chunks),
                    ms: Date.now() - t0,
                }));
            }
        );
        req.on('error', reject);
        req.end();
    });
}

(async () => {
    const disk = fs.readFileSync(WASM);
    const diskSha = sha(disk);
    console.log(`wasm su disco: ${disk.length.toLocaleString()} byte  sha256=${diskSha.slice(0, 16)}…`);
    console.log('');

    const wasmPath = `/gravity_sandbox_bg.wasm?v=${VERSION}`;

    // --- brotli: quello che chiede Safari ---
    const br = await rawGet(wasmPath, { 'Accept-Encoding': 'gzip, deflate, br' });
    check('stato 200 (br)', br.status === 200, String(br.status));
    check('Content-Encoding: br', br.headers['content-encoding'] === 'br', br.headers['content-encoding']);
    check('Content-Type application/wasm', br.headers['content-type'] === 'application/wasm',
        br.headers['content-type']);
    check('cache immutable su ?v=', /immutable/.test(br.headers['cache-control'] || ''),
        br.headers['cache-control']);
    check('Vary: Accept-Encoding', /accept-encoding/i.test(br.headers['vary'] || ''), br.headers['vary']);
    check('Content-Length == byte ricevuti', String(br.body.length) === br.headers['content-length'],
        `${br.body.length} vs ${br.headers['content-length']}`);
    let brOk = false;
    let brDecoded = 0;
    try {
        const decoded = zlib.brotliDecompressSync(br.body);
        brDecoded = decoded.length;
        brOk = sha(decoded) === diskSha;
    } catch (e) {
        console.log('   errore decompressione br:', e.message);
    }
    check('brotli decomprime identico al wasm su disco', brOk, `${brDecoded.toLocaleString()} byte`);
    console.log(`   brotli trasferito: ${br.body.length.toLocaleString()} byte `
        + `(${(br.body.length / 1024 / 1024).toFixed(2)} MB, ${((br.body.length / disk.length) * 100).toFixed(1)}% del grezzo)`);

    // --- gzip: fallback per i client senza brotli ---
    const gz = await rawGet(wasmPath, { 'Accept-Encoding': 'gzip, deflate' });
    check('Content-Encoding: gzip', gz.headers['content-encoding'] === 'gzip', gz.headers['content-encoding']);
    let gzOk = false;
    try {
        gzOk = sha(zlib.gunzipSync(gz.body)) === diskSha;
    } catch (e) {
        console.log('   errore decompressione gz:', e.message);
    }
    check('gzip decomprime identico al wasm su disco', gzOk);
    console.log(`   gzip trasferito:   ${gz.body.length.toLocaleString()} byte `
        + `(${(gz.body.length / 1024 / 1024).toFixed(2)} MB)`);

    // --- nessuna compressione accettata: deve arrivare il grezzo ---
    const plain = await rawGet(wasmPath, { 'Accept-Encoding': 'identity' });
    check('senza encoding: nessun Content-Encoding', !plain.headers['content-encoding'],
        String(plain.headers['content-encoding']));
    check('senza encoding: identico al disco', sha(plain.body) === diskSha);

    // --- sidecar della taglia decompressa (progress bar) ---
    const sizeRes = await rawGet(`/gravity_sandbox_bg.wasm.size?v=${VERSION}`);
    check('.size == dimensione decompressa', parseInt(sizeRes.body.toString().trim(), 10) === disk.length,
        `${sizeRes.body.toString().trim()} vs ${disk.length}`);

    // --- index.html servito con la versione corrente ---
    const html = (await rawGet('/index.html')).body.toString();
    check('index.html cita la versione corrente', html.includes(`?v=${VERSION}`), VERSION);
    check('index.html non ha badge stale',
        !new RegExp(`version-badge">v(?!${VERSION})`).test(html));

    // --- asset leggeri: la cache segue la stessa regola ---
    const js = await rawGet(`/gravity_sandbox.js?v=${VERSION}`, { 'Accept-Encoding': 'br' });
    check('glue JS versionato: immutable', /immutable/.test(js.headers['cache-control'] || ''),
        js.headers['cache-control']);
    const htmlCache = await rawGet('/index.html');
    check('index.html (non versionato): no-cache', htmlCache.headers['cache-control'] === 'no-cache',
        htmlCache.headers['cache-control']);

    console.log('');
    console.log(failures === 0 ? 'OK — tutto verificato' : `${failures} CONTROLLI FALLITI`);
    process.exit(failures === 0 ? 0 : 1);
})().catch((e) => {
    console.error('ERRORE:', e.message);
    process.exit(1);
});
