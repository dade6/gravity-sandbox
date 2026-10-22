#!/usr/bin/env node
// Sanita' del wasm DOPO lo strip: il file deve restare valido e conservare
// tutti gli export che il glue JS (gravity_sandbox.d.ts) si aspetta.
//
// Uso: node scripts/verify_wasm.js [percorso.wasm]
const fs = require('fs');
const path = require('path');

const file = process.argv[2] || path.join(__dirname, '..', 'wasm-dist', 'gravity_sandbox_bg.wasm');
const dts = path.join(path.dirname(file), 'gravity_sandbox.d.ts');

const bytes = fs.readFileSync(file);
let failures = 0;
const check = (name, cond, detail) => {
    if (!cond) failures++;
    console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${detail ? '  [' + detail + ']' : ''}`);
};

check('file letto', bytes.length > 0, `${(bytes.length / 1024 / 1024).toFixed(2)} MB`);
check('WebAssembly.validate', WebAssembly.validate(bytes));

let mod;
try {
    mod = new WebAssembly.Module(bytes);
} catch (e) {
    console.log('FAIL  compilazione modulo:', e.message);
    process.exit(1);
}

const exports_ = WebAssembly.Module.exports(mod).map((e) => e.name);
const imports_ = WebAssembly.Module.imports(mod);
console.log(`   export: ${exports_.length}, import: ${imports_.length}`);
check('il modulo espone funzioni', exports_.length > 0);

// Ogni funzione dichiarata nel .d.ts deve esistere: o come export del wasm,
// o come funzione del glue JS accanto (wasm-bindgen definisce nel glue le
// funzioni di bootstrap, es. initSync).
if (fs.existsSync(dts)) {
    const src = fs.readFileSync(dts, 'utf8');
    const declared = [...src.matchAll(/export function ([A-Za-z0-9_]+)/g)].map((m) => m[1]);
    const gluePath = path.join(path.dirname(file), 'gravity_sandbox.js');
    const glue = fs.existsSync(gluePath) ? fs.readFileSync(gluePath, 'utf8') : '';
    const missing = declared.filter((n) => !exports_.includes(n)
        && !new RegExp(`function ${n}\\b|\\b${n}\\b as`).test(glue));
    check('export del .d.ts presenti (wasm o glue)', missing.length === 0,
        missing.length ? 'mancano: ' + missing.slice(0, 5).join(', ') : `${declared.length} dichiarati`);
} else {
    console.log('   (nessun .d.ts accanto al wasm: salto il confronto degli export)');
}

// La memoria esportata serve al glue per allocare i buffer.
check('memoria esportata', exports_.includes('memory') || imports_.some((i) => i.kind === 'memory'));

console.log('');
console.log(failures === 0 ? 'OK — wasm valido' : `${failures} CONTROLLI FALLITI`);
process.exit(failures === 0 ? 0 : 1);
