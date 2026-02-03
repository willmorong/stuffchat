const { app, BrowserWindow, shell, desktopCapturer, ipcMain, session, Notification, systemPreferences } = require('electron');
const path = require('path');

const STUFFCHAT_URL = 'https://chat.stuffcity.org';

let mainWindow;

// Platform-specific optimizations
if (process.platform === 'linux') {
    app.commandLine.appendSwitch('ignore-gpu-blocklist');
    app.commandLine.appendSwitch('enable-gpu-rasterization');
    app.commandLine.appendSwitch('enable-zero-copy');
    app.commandLine.appendSwitch('disable-renderer-backgrounding');
    app.commandLine.appendSwitch('disable-background-timer-throttling');
    app.commandLine.appendSwitch('enable-features', 'VaapiVideoDecoder,VaapiVideoEncoder,WebRTCPipeWireCapturer');
} else if (process.platform === 'darwin') {
    // macOS specific optimizations if any (VideoToolbox is default)
    app.commandLine.appendSwitch('disable-renderer-backgrounding');
    app.commandLine.appendSwitch('disable-background-timer-throttling');
} else if (process.platform === 'win32') {
    // Windows specific optimizations
    app.commandLine.appendSwitch('ignore-gpu-blocklist');
    app.commandLine.appendSwitch('enable-gpu-rasterization');
    app.commandLine.appendSwitch('enable-zero-copy');
    app.commandLine.appendSwitch('disable-renderer-backgrounding');
    app.commandLine.appendSwitch('disable-background-timer-throttling');
}
app.commandLine.appendSwitch('force-fieldtrials', 'WebRTC-FlexFEC-03/Enabled/');

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

// Handle microphone permission status/request
ipcMain.handle('get-mic-permission', async () => {
    if (process.platform !== 'darwin') return 'granted';
    return systemPreferences.getMediaAccessStatus('microphone');
});

ipcMain.handle('request-mic-permission', async () => {
    if (process.platform !== 'darwin') return true;
    return await systemPreferences.askForMediaAccess('microphone');
});

app.whenReady().then(() => {
    // Auto-grant notification and media permissions where appropriate
    session.defaultSession.setPermissionRequestHandler((webContents, permission, callback) => {
        const url = webContents.getURL();
        if (!url.startsWith(STUFFCHAT_URL)) {
            return callback(false); // Only grant permissions to our trusted URL
        }

        if (permission === 'notifications' || permission === 'media') {
            callback(true);
        } else {
            callback(true); // Grant other permissions as needed
        }
    });

    // Check and request microphone permission on macOS startup
    if (process.platform === 'darwin') {
        const micStatus = systemPreferences.getMediaAccessStatus('microphone');
        if (micStatus === 'not-determined') {
            systemPreferences.askForMediaAccess('microphone').then(granted => {
                console.log(`Initial microphone permission: ${granted}`);
            });
        }
    }

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
