/**
 * Shared utility for managing the background canvas across animated themes
 * Handles recreating the canvas element to switch between 2D and WebGL contexts
 */

let currentTheme = null;

/**
 * Recreates the background-canvas element to ensure a fresh context
 * This is necessary because a canvas can't switch between 2D and WebGL contexts
 */
export function recreateCanvas() {
    const oldCanvas = document.getElementById('background-canvas');
    if (!oldCanvas) return null;

    // Create a new canvas with the same attributes
    const newCanvas = document.createElement('canvas');
    newCanvas.id = 'background-canvas';
    newCanvas.width = window.innerWidth;
    newCanvas.height = window.innerHeight;
    newCanvas.style.display = 'none';

    // Replace the old canvas
    oldCanvas.parentNode.replaceChild(newCanvas, oldCanvas);

    return newCanvas;
}

/**
 * Get or create the background canvas
 */
export function getCanvas() {
    let canvas = document.getElementById('background-canvas');
    if (!canvas) {
        canvas = document.createElement('canvas');
        canvas.id = 'background-canvas';
        document.body.insertBefore(canvas, document.body.firstChild);
    }
    return canvas;
}

/**
 * Track the current active animated theme
 */
export function setCurrentTheme(theme) {
    currentTheme = theme;
}

export function getCurrentTheme() {
    return currentTheme;
}
