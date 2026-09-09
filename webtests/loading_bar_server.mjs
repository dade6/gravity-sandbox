// webtests/loading_bar_server.mjs — fixture server DEDICATO ai test del
// componente loading_bar.js (card t_63406e55). Autocontenuto: NON tocca
// server.mjs (t_d212617b) né compile_server.mjs (t_3c91ee24) per evitare
// conflitti tra card parallele.
//
//   /loading_bar.js                  modulo sotto test (repo root)
//   /loading-bar-demo.html           demo interattiva (repo root)
//   /loading-bar-test-page.html      pagina host dei test (webtests/)
//   /shutdown POST                   spegne il server
import http from 'node:http';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const PORT = parseInt(process.argv[2] || '8185', 10);
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const FILES = {
    '/loading_bar.js': { file: 'loading_bar.js', type: 'application/javascript' },
    '/loading-bar-demo.html': { file: 'loading-bar-demo.html', type: 'text/html' },
    '/loading-bar-test-page.html': { file: 'webtests/loading-bar-test-page.html', type: 'text/html' },
};

const server = http.createServer((req, res) => {
    const p = req.url.split('?')[0];
    const entry = FILES[p];
    if (entry) {
        try {
            const body = readFileSync(path.join(REPO, entry.file));
            res.writeHead(200, {
                'Content-Type': entry.type,
                'Cache-Control': 'no-store',
                'Access-Control-Allow-Origin': '*',
            });
            res.end(body);
            return;
        } catch (e) {
            res.writeHead(500, { 'Content-Type': 'text/plain' });
            res.end('read error: ' + e.message);
            return;
        }
    }
    if (p === '/shutdown' && req.method === 'POST') {
        res.writeHead(204);
        res.end();
        setTimeout(() => process.exit(0), 50);
    } else {
        res.writeHead(404, { 'Content-Type': 'text/plain' });
        res.end('not found: ' + p);
    }
});
server.listen(PORT, '127.0.0.1', () => {
    console.log(`[loading-bar-server] fixture server su http://127.0.0.1:${PORT}`);
});
