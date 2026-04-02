import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import type { Plugin } from 'vite';
import { readFileSync } from 'fs';
import { request as httpRequest } from 'http';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
// BUILD_WORKSPACE_DIRECTORY is set by `bazel run`; fall back to relative __dirname
const projectRoot = process.env.BUILD_WORKSPACE_DIRECTORY ?? join(__dirname, '..', '..');

function getDataDir(): string {
  return process.env.WORKSHOP_DATA_DIR ?? join(projectRoot, 'local', 'state', 'dev');
}

// Re-reads daemon.port on each call so the proxy follows backend restarts.
// Cached for 2s to avoid excessive filesystem reads.
let cachedPort = '';
let cachedAt = 0;

function getDaemonPort(): string {
  const now = Date.now();
  if (cachedPort && now - cachedAt < 2000) return cachedPort;

  const dataDir = getDataDir();
  const candidates = [join(dataDir, 'state', 'daemon.port'), join(dataDir, 'daemon.port')];
  for (const portFile of candidates) {
    try {
      const port = readFileSync(portFile, 'utf-8').trim();
      cachedPort = port;
      cachedAt = now;
      return port;
    } catch {
      // try next
    }
  }
  return '3000';
}

/**
 * Vite plugin that proxies /api to the backend, re-reading daemon.port on
 * each request so the proxy automatically follows backend restarts.
 */
function dynamicBackendProxy(): Plugin {
  return {
    name: 'dynamic-backend-proxy',
    configureServer(server) {
      // HTTP requests
      server.middlewares.use((req, res, next) => {
        if (!req.url?.startsWith('/api')) return next();

        const port = parseInt(getDaemonPort(), 10);
        const proxyReq = httpRequest(
          {
            hostname: 'localhost',
            port,
            path: req.url,
            method: req.method,
            headers: req.headers
          },
          (proxyRes) => {
            res.writeHead(proxyRes.statusCode ?? 502, proxyRes.headers);
            proxyRes.pipe(res);
          }
        );
        proxyReq.on('error', (err) => {
          console.error('[proxy]', err.message);
          if (!res.headersSent) {
            res.writeHead(502, { 'Content-Type': 'text/plain' });
          }
          res.end('Backend unavailable');
        });
        req.pipe(proxyReq);
      });

      // WebSocket upgrades
      server.httpServer?.on('upgrade', (req, socket, head) => {
        if (!req.url?.startsWith('/api')) return;

        const port = parseInt(getDaemonPort(), 10);
        const proxyReq = httpRequest({
          hostname: 'localhost',
          port,
          path: req.url,
          method: 'GET',
          headers: req.headers
        });
        proxyReq.on('upgrade', (_proxyRes, proxySocket, proxyHead) => {
          socket.write(
            'HTTP/1.1 101 Switching Protocols\r\n' +
              Object.entries(_proxyRes.headers)
                .map(([k, v]) => `${k}: ${v}`)
                .join('\r\n') +
              '\r\n\r\n'
          );
          if (proxyHead.length > 0) socket.write(proxyHead);
          proxySocket.pipe(socket);
          socket.pipe(proxySocket);
        });
        proxyReq.on('error', (err) => {
          console.error('[proxy ws]', err.message);
          socket.destroy();
        });
        proxyReq.end(head);
      });
    }
  };
}

export default defineConfig({
  plugins: [sveltekit(), dynamicBackendProxy()],
  server: {
    // Allow Bazel runfiles tree access
    fs: {
      strict: false
    }
  }
});
