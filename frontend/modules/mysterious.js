/**
 * Mysterious theme animation - Smooth lava-lamp style with Perlin noise
 */

// Perlin noise implementation
class PerlinNoise {
    constructor() {
        this.permutation = [];
        for (let i = 0; i < 256; i++) {
            this.permutation[i] = i;
        }
        // Shuffle
        for (let i = 255; i > 0; i--) {
            const j = Math.floor(Math.random() * (i + 1));
            [this.permutation[i], this.permutation[j]] = [this.permutation[j], this.permutation[i]];
        }
        // Duplicate
        this.permutation = [...this.permutation, ...this.permutation];
    }

    fade(t) {
        return t * t * t * (t * (t * 6 - 15) + 10);
    }

    lerp(a, b, t) {
        return a + t * (b - a);
    }

    grad(hash, x, y) {
        const h = hash & 3;
        const u = h < 2 ? x : y;
        const v = h < 2 ? y : x;
        return ((h & 1) === 0 ? u : -u) + ((h & 2) === 0 ? v : -v);
    }

    noise(x, y) {
        const X = Math.floor(x) & 255;
        const Y = Math.floor(y) & 255;

        x -= Math.floor(x);
        y -= Math.floor(y);

        const u = this.fade(x);
        const v = this.fade(y);

        const A = this.permutation[X] + Y;
        const B = this.permutation[X + 1] + Y;

        return this.lerp(
            this.lerp(
                this.grad(this.permutation[A], x, y),
                this.grad(this.permutation[B], x - 1, y),
                u
            ),
            this.lerp(
                this.grad(this.permutation[A + 1], x, y - 1),
                this.grad(this.permutation[B + 1], x - 1, y - 1),
                u
            ),
            v
        );
    }

    // Octave noise for smoother, more organic appearance
    octaveNoise(x, y, octaves = 4, persistence = 0.5) {
        let total = 0;
        let frequency = 1;
        let amplitude = 1;
        let maxValue = 0;

        for (let i = 0; i < octaves; i++) {
            total += this.noise(x * frequency, y * frequency) * amplitude;
            maxValue += amplitude;
            amplitude *= persistence;
            frequency *= 2;
        }

        return total / maxValue;
    }
}

// Animation state
let canvas = null;
let ctx = null;
let animationId = null;
let perlin = null;
let perlin2 = null; // Second noise layer for more organic movement
let timeOffset = 0;
let imageData = null;

// Vibrant colors for mysterious theme
const COLORS = [
    { r: 0, g: 0, b: 0 },      // Black (--bg1)
    { r: 27, g: 38, b: 59 },      // Mid blue (--bg2)
    { r: 60, g: 50, b: 120 },     // Purple accent
    { r: 50, g: 30, b: 75 },     // Darker purple
    { r: 100, g: 150, b: 255 },   // Vibrant blue (--accent)
    { r: 35, g: 120, b: 85 }     // Dark teal (--accent-2)
];

// Pixel step for performance (render every Nth pixel, interpolate the rest)
const PIXEL_STEP = 4;

function lerpColor(c1, c2, t) {
    return {
        r: c1.r + (c2.r - c1.r) * t,
        g: c1.g + (c2.g - c1.g) * t,
        b: c1.b + (c2.b - c1.b) * t
    };
}

function getColorFromNoise(t) {
    // t is 0-1, map to color stops
    const stops = COLORS.length - 1;
    const scaledT = t * stops;
    const index = Math.floor(scaledT);
    const frac = scaledT - index;

    if (index >= stops) {
        return COLORS[stops];
    }

    return lerpColor(COLORS[index], COLORS[index + 1], frac);
}

function render() {
    if (!canvas || !ctx || !perlin || !perlin2) return;

    const width = canvas.width;
    const height = canvas.height;

    // Create or reuse imageData
    if (!imageData || imageData.width !== width || imageData.height !== height) {
        imageData = ctx.createImageData(width, height);
    }

    const data = imageData.data;

    // Noise parameters for lava-lamp effect
    const baseScale = 0.003;      // Large, smooth blobs
    const detailScale = 0.008;    // Finer detail layer
    const timeSpeed = 0.0001;     // Slow, hypnotic movement

    // Calculate grid dimensions for sampling
    const gridWidth = Math.floor(width / PIXEL_STEP) + 1;
    const gridHeight = Math.floor(height / PIXEL_STEP) + 1;

    // Sample at lower resolution for performance
    const tempColors = new Float32Array(gridWidth * gridHeight * 3);

    let tempIndex = 0;
    for (let gy = 0; gy < gridHeight; gy++) {
        for (let gx = 0; gx < gridWidth; gx++) {
            const x = gx * PIXEL_STEP;
            const y = gy * PIXEL_STEP;

            // Layer 1: Large flowing blobs
            const noise1 = perlin.octaveNoise(
                x * baseScale + timeOffset,
                y * baseScale + timeOffset * 0.6,
                3,
                0.6
            );

            // Layer 2: Secondary movement with different direction
            const noise2 = perlin2.octaveNoise(
                x * baseScale * 0.7 - timeOffset * 0.8,
                y * baseScale * 0.7 + timeOffset * 0.4,
                2,
                0.5
            );

            // Layer 3: Fine detail for organic texture
            const noise3 = perlin.octaveNoise(
                x * detailScale + timeOffset * 1.5,
                y * detailScale - timeOffset * 0.3,
                2,
                0.4
            );

            // Combine noise layers with different weights
            const combined = (noise1 * 0.5 + noise2 * 0.35 + noise3 * 0.15);

            // Apply smoothstep for more defined blob edges (lava lamp effect)
            let t = (combined + 1) / 2;
            t = t * t * (3 - 2 * t); // Smoothstep

            const color = getColorFromNoise(t);
            tempColors[tempIndex++] = color.r;
            tempColors[tempIndex++] = color.g;
            tempColors[tempIndex++] = color.b;
        }
    }

    // Fill in pixels with bilinear interpolation
    for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x++) {
            const sampleX = x / PIXEL_STEP;
            const sampleY = y / PIXEL_STEP;

            const x0 = Math.floor(sampleX);
            const y0 = Math.floor(sampleY);
            const x1 = Math.min(x0 + 1, gridWidth - 1);
            const y1 = Math.min(y0 + 1, gridHeight - 1);

            const fx = sampleX - x0;
            const fy = sampleY - y0;

            const idx00 = (y0 * gridWidth + x0) * 3;
            const idx10 = (y0 * gridWidth + x1) * 3;
            const idx01 = (y1 * gridWidth + x0) * 3;
            const idx11 = (y1 * gridWidth + x1) * 3;

            // Bilinear interpolation
            const r = (tempColors[idx00] * (1 - fx) + tempColors[idx10] * fx) * (1 - fy) +
                (tempColors[idx01] * (1 - fx) + tempColors[idx11] * fx) * fy;
            const g = (tempColors[idx00 + 1] * (1 - fx) + tempColors[idx10 + 1] * fx) * (1 - fy) +
                (tempColors[idx01 + 1] * (1 - fx) + tempColors[idx11 + 1] * fx) * fy;
            const b = (tempColors[idx00 + 2] * (1 - fx) + tempColors[idx10 + 2] * fx) * (1 - fy) +
                (tempColors[idx01 + 2] * (1 - fx) + tempColors[idx11 + 2] * fx) * fy;

            const pixelIndex = (y * width + x) * 4;
            data[pixelIndex] = r;
            data[pixelIndex + 1] = g;
            data[pixelIndex + 2] = b;
            data[pixelIndex + 3] = 255;
        }
    }

    ctx.putImageData(imageData, 0, 0);

    // Update time offset for animation
    timeOffset += timeSpeed * 16;

    // Continue animation
    animationId = requestAnimationFrame(render);
}

function resizeCanvas() {
    if (!canvas) return;
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    imageData = null; // Reset imageData on resize
}

export function startMysteriousAnimation() {
    canvas = document.getElementById('background-canvas');
    if (!canvas) return;

    ctx = canvas.getContext('2d');
    perlin = new PerlinNoise();
    perlin2 = new PerlinNoise(); // Second noise generator for variation

    resizeCanvas();
    window.addEventListener('resize', resizeCanvas);

    // Show canvas
    canvas.style.display = 'block';

    // Start animation
    if (!animationId) {
        render();
    }
}

export function stopMysteriousAnimation() {
    // Cancel animation if running
    if (animationId) {
        cancelAnimationFrame(animationId);
        animationId = null;
    }

    // Remove resize listener
    window.removeEventListener('resize', resizeCanvas);

    // Hide canvas
    if (canvas) {
        canvas.style.display = 'none';
    }

    // Reset all module state
    canvas = null;
    ctx = null;
    perlin = null;
    perlin2 = null;
    imageData = null;
}

