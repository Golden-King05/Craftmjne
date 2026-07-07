// Serves the built game (dist/) over a secure custom app:// scheme.
// This is the Electron-documented pattern (protocol.handle + net.fetch) and
// avoids the file:// protocol's restrictions around workers, fetch and
// strict-MIME module scripts.

const { protocol, net } = require('electron');
const path = require('node:path');
const { pathToFileURL } = require('node:url');

const DIST = path.join(__dirname, '../dist');
const APP_URL = 'app://craftmjne/';

// Must run before app.whenReady().
function registerScheme() {
  protocol.registerSchemesAsPrivileged([
    {
      scheme: 'app',
      privileges: {
        standard: true,
        secure: true,
        supportFetchAPI: true,
        stream: true,
      },
    },
  ]);
}

// Must run after app.whenReady().
function attachHandler() {
  protocol.handle('app', (request) => {
    const { pathname } = new URL(request.url);
    const rel = decodeURIComponent(pathname === '/' ? '/index.html' : pathname);
    const file = path.normalize(path.join(DIST, rel));
    if (!file.startsWith(DIST + path.sep) && file !== path.join(DIST, 'index.html')) {
      return new Response('Forbidden', { status: 403 });
    }
    return net.fetch(pathToFileURL(file).toString());
  });
}

module.exports = { registerScheme, attachHandler, APP_URL };
