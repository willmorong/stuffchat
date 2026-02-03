const { app, BrowserWindow, shell, desktopCapturer, ipcMain, session, Notification } = require('electron');
const path = require('path');

const STUFFCHAT_URL = 'https://chat.stuffcity.org';

let mainWindow;

function createWindow() {
    mainWindow = new BrowserWindow({
        width: 1200,
        height: 800,
        minWidth: 800,
        minHeight: 600,
        title: 'stuffchat',
        icon: path.join(__dirname, 'icon.png'),
        webPreferences: {
            preload: path.join(__dirname, 'preload.js'),
            nodeIntegration: false,
            contextIsolation: true,
        },
        autoHideMenuBar: true,
    });

    mainWindow.loadURL(STUFFCHAT_URL);

    // Open external links in the default browser
    mainWindow.webContents.setWindowOpenHandler(({ url }) => {
        if (!url.startsWith(STUFFCHAT_URL)) {
            shell.openExternal(url);
            return { action: 'deny' };
        }
        return { action: 'allow' };
    });

    // Handle navigation to external URLs
    mainWindow.webContents.on('will-navigate', (event, url) => {
        if (!url.startsWith(STUFFCHAT_URL)) {
            event.preventDefault();
            shell.openExternal(url);
        }
    });

    mainWindow.on('closed', () => {
        mainWindow = null;
    });

    // Handle notification clicks - focus the window
    mainWindow.webContents.on('notification-click', () => {
        if (mainWindow) {
            if (mainWindow.isMinimized()) mainWindow.restore();
            mainWindow.focus();
        }
    });
}

// Handle screen sharing source selection
ipcMain.handle('get-sources', async () => {
    const sources = await desktopCapturer.getSources({
        types: ['window', 'screen'],
        thumbnailSize: { width: 150, height: 150 },
        fetchWindowIcons: true
    });
    return sources.map(source => ({
        id: source.id,
        name: source.name,
        thumbnail: source.thumbnail.toDataURL(),
        appIcon: source.appIcon ? source.appIcon.toDataURL() : null
    }));
});

// Handle native notifications with app icon
ipcMain.handle('show-notification', (event, title, body) => {
    const notification = new Notification({
        title: title,
        body: body,
        icon: path.join(__dirname, 'icon.png')
    });

    notification.on('click', () => {
        if (mainWindow) {
            if (mainWindow.isMinimized()) mainWindow.restore();
            mainWindow.focus();
        }
    });

    notification.show();
    return true;
});

app.whenReady().then(() => {
    // Auto-grant notification permissions
    session.defaultSession.setPermissionRequestHandler((webContents, permission, callback) => {
        if (permission === 'notifications') {
            callback(true);
        } else {
            callback(true); // Grant other permissions as needed
        }
    });

    // Handle getDisplayMedia requests for screen sharing
    session.defaultSession.setDisplayMediaRequestHandler((request, callback) => {
        desktopCapturer.getSources({ types: ['screen', 'window'] }).then((sources) => {
            // If there's only one source, use it directly
            if (sources.length === 1) {
                callback({ video: sources[0], audio: 'loopback' });
            } else {
                // Send sources to renderer for user selection
                mainWindow.webContents.send('select-source', sources.map(s => ({
                    id: s.id,
                    name: s.name,
                    thumbnail: s.thumbnail.toDataURL()
                })));

                // Listen for user's selection
                ipcMain.once('source-selected', (event, sourceId) => {
                    const source = sources.find(s => s.id === sourceId);
                    if (source) {
                        callback({ video: source, audio: 'loopback' });
                    } else {
                        callback({});
                    }
                });
            }
        });
    }, { useSystemPicker: false });

    createWindow();

    app.on('activate', () => {
        if (BrowserWindow.getAllWindows().length === 0) {
            createWindow();
        }
    });
});

app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') {
        app.quit();
    }
});
