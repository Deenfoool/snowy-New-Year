// Local development preview of the canonical Island Lab browser UI.
// Node.js built-ins only. Serves an explicit asset allowlist on loopback.
import http from 'node:http';
import { readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const assets = Object.freeze({
  '/': ['index.html', 'text/html; charset=utf-8'],
  '/index.html': ['index.html', 'text/html; charset=utf-8'],
  '/styles.css': ['styles.css', 'text/css; charset=utf-8'],
  '/app.js': ['app.js', 'application/javascript; charset=utf-8'],
  '/motion.js': ['motion.js', 'application/javascript; charset=utf-8'],
  '/system-media.js': ['system-media.js', 'application/javascript; charset=utf-8'],
  '/audio-visualizer.js': ['audio-visualizer.js', 'application/javascript; charset=utf-8'],
  '/desktop-mode.js': ['desktop-mode.js', 'application/javascript; charset=utf-8']
});

export function getPreviewAsset(requestUrl) {
  try {
    const route = new URL(requestUrl, 'http://localhost').pathname;
    return Object.hasOwn(assets, route) ? assets[route] : null;
  } catch {
    return null;
  }
}

export function createPreviewServer() {
  return http.createServer(async (req, res) => {
    if (req.method !== 'GET' && req.method !== 'HEAD') {
      res.writeHead(405, { Allow: 'GET, HEAD' });
      res.end();
      return;
    }
    const asset = getPreviewAsset(req.url || '/');
    if (!asset) {
      res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
      res.end('Not found');
      return;
    }
    try {
      const body = await readFile(join(root, asset[0]));
      res.writeHead(200, {
        'Content-Type': asset[1],
        'Cache-Control': 'no-store',
        'X-Content-Type-Options': 'nosniff'
      });
      res.end(req.method === 'HEAD' ? undefined : body);
    } catch (error) {
      console.error('Island Lab preview failed to read an asset:', error);
      res.writeHead(500, { 'Content-Type': 'text/plain; charset=utf-8' });
      res.end('Local preview asset unavailable');
    }
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const server = createPreviewServer();
  const requestedPort = Number(process.env.PORT) || 4173;
  server.on('error', error => {
    console.error('Island Lab preview cannot start:', error.message);
    process.exitCode = 1;
  });
  server.listen(requestedPort, '127.0.0.1', () => {
    console.log('Island Lab local preview: http://127.0.0.1:' + requestedPort + '/');
    console.log('Stop the preview with Ctrl+C. The Windows-native features are not available in this browser build.');
  });
}
