const { contextBridge, ipcRenderer } = require('electron');

// Expose screen sharing APIs to the renderer
contextBridge.exposeInMainWorld('electronAPI', {
    // Get available screen/window sources for sharing
    getSources: () => ipcRenderer.invoke('get-sources'),

    // Listen for source selection request from main process
    onSelectSource: (callback) => {
        ipcRenderer.on('select-source', (event, sources) => callback(sources));
    },

    // Send selected source back to main process
    selectSource: (sourceId) => ipcRenderer.send('source-selected', sourceId),

    // Show a native notification
    showNotification: (title, body) => ipcRenderer.invoke('show-notification', title, body),

    // Check if running in Electron
    isElectron: true
});

window.addEventListener('DOMContentLoaded', () => {
    console.log('Stuffchat Desktop loaded with screen sharing support');
});
