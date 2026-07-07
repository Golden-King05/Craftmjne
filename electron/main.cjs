// Electron main process: creates the game window.
// In development (npm run dev) it loads the Vite dev server for HMR;
// in production it serves the built dist/ over a secure app:// scheme.

const { app, BrowserWindow, shell, globalShortcut } = require('electron');
const { registerScheme, attachHandler, APP_URL } = require('./serve.cjs');

const DEV_SERVER_URL = process.env.VITE_DEV_SERVER_URL;

if (!DEV_SERVER_URL) registerScheme();

function createWindow() {
  const win = new BrowserWindow({
    width: 1280,
    height: 720,
    minWidth: 640,
    minHeight: 480,
    title: 'Craftmjne',
    backgroundColor: '#0a0e14',
    autoHideMenuBar: true,
    show: false,
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });

  win.once('ready-to-show', () => win.show());

  // The game is fully self-contained; open any external link in the OS browser.
  win.webContents.setWindowOpenHandler(({ url }) => {
    shell.openExternal(url);
    return { action: 'deny' };
  });

  win.loadURL(DEV_SERVER_URL ?? APP_URL);
  return win;
}

app.whenReady().then(() => {
  if (!DEV_SERVER_URL) attachHandler();
  const win = createWindow();

  globalShortcut.register('F11', () => {
    const w = BrowserWindow.getFocusedWindow() ?? win;
    w.setFullScreen(!w.isFullScreen());
  });
  if (DEV_SERVER_URL) {
    globalShortcut.register('F12', () => {
      (BrowserWindow.getFocusedWindow() ?? win).webContents.toggleDevTools();
    });
  }

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on('will-quit', () => globalShortcut.unregisterAll());

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});
