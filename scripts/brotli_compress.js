// Comprime un file con brotli q11 (wasm: brotli batte gzip di ~20-25%).
// Uso: node brotli_compress.js <input> [output]
const fs = require('fs');
const zlib = require('zlib');

const input = process.argv[2];
const output = process.argv[3] || input + '.br';
const data = fs.readFileSync(input);
const t0 = Date.now();
const out = zlib.brotliCompressSync(data, {
    params: {
        [zlib.constants.BROTLI_PARAM_MODE]: zlib.constants.BROTLI_MODE_GENERIC,
        [zlib.constants.BROTLI_PARAM_QUALITY]: 11,
        [zlib.constants.BROTLI_PARAM_LGWIN]: 24,
        [zlib.constants.BROTLI_PARAM_SIZE_HINT]: data.length,
    },
});
fs.writeFileSync(output, out);
console.log(
    input + ': ' + data.length.toLocaleString() + ' -> ' + out.length.toLocaleString() +
    ' (' + (out.length / 1024 / 1024).toFixed(2) + ' MB, ' +
    ((out.length / data.length) * 100).toFixed(1) + '%, ' +
    ((Date.now() - t0) / 1000).toFixed(1) + 's)'
);
