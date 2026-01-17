/**
 * Clouds theme animation - Perlin noise hexagonal tiles
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

    // Octave noise for more natural cloud-like appearance
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
let timeOffset = 0;

// Colors for clouds theme
const COLORS = {
    lightBlue: { r: 135, g: 206, b: 250 },   // Light sky blue
    white: { r: 255, g: 255, b: 255 },        // White
    skyBlue: { r: 100, g: 180, b: 255 },      // Sky blue
    deepBlue: { r: 70, g: 130, b: 200 }       // Deeper blue
};

// Hexagon settings
const HEX_SIZE = 12; // Radius of hexagon

function getHexPoints(cx, cy, size) {
    const points = [];
    for (let i = 0; i < 6; i++) {
        const angle = (Math.PI / 3) * i - Math.PI / 6;
        points.push({
            x: cx + size * Math.cos(angle),
            y: cy + size * Math.sin(angle)
        });
    }
    return points;
}

function drawHexagon(ctx, cx, cy, size, color) {
    const points = getHexPoints(cx, cy, size);
    ctx.beginPath();
    ctx.moveTo(points[0].x, points[0].y);
    for (let i = 1; i < 6; i++) {
        ctx.lineTo(points[i].x, points[i].y);
    }
    ctx.closePath();
    ctx.fillStyle = color;
    ctx.fill();
}

function lerpColor(c1, c2, t) {
    return {
        r: Math.round(c1.r + (c2.r - c1.r) * t),
        g: Math.round(c1.g + (c2.g - c1.g) * t),
        b: Math.round(c1.b + (c2.b - c1.b) * t)
    };
}

function colorToString(c, alpha = 1) {
    return `rgba(${c.r}, ${c.g}, ${c.b}, ${alpha})`;
}

function render() {
    if (!canvas || !ctx || !perlin) return;

    const width = canvas.width;
    const height = canvas.height;

    // Clear with gradient background
    const gradient = ctx.createLinearGradient(0, 0, 0, height);
    gradient.addColorStop(0, '#87CEEB');   // Light sky blue
    gradient.addColorStop(1, '#B0E0E6');   // Powder blue
    ctx.fillStyle = gradient;
    ctx.fillRect(0, 0, width, height);

    // Hexagon grid dimensions
    const hexWidth = HEX_SIZE * Math.sqrt(3);
    const hexHeight = HEX_SIZE * 2;
    const vertSpacing = hexHeight * 0.75;

    const cols = Math.ceil(width / hexWidth) + 2;
    const rows = Math.ceil(height / vertSpacing) + 2;

    // Scale and speed for noise
    const noiseScale = 0.015;
    const timeSpeed = 0.0002; // Slowing it down slightly too as larger patterns move faster visually

    for (let row = -1; row < rows; row++) {
        for (let col = -1; col < cols; col++) {
            // Offset every other row
            const xOffset = (row % 2) * (hexWidth / 2);
            const cx = col * hexWidth + xOffset;
            const cy = row * vertSpacing;

            // Get noise value for this position
            const noiseVal = perlin.octaveNoise(
                cx * noiseScale + timeOffset,
                cy * noiseScale + timeOffset * 0.5,
                3,
                0.5
            );

            // Map noise to 0-1 range
            const t = (noiseVal + 1) / 2;

            // Blend between colors based on noise
            let color;
            if (t < 0.4) {
                // Deep blue to sky blue
                color = lerpColor(COLORS.deepBlue, COLORS.skyBlue, t / 0.4);
            } else if (t < 0.7) {
                // Sky blue to light blue
                color = lerpColor(COLORS.skyBlue, COLORS.lightBlue, (t - 0.4) / 0.3);
            } else {
                // Light blue to white (clouds)
                color = lerpColor(COLORS.lightBlue, COLORS.white, (t - 0.7) / 0.3);
            }

            drawHexagon(ctx, cx, cy, HEX_SIZE, colorToString(color, 0.85));
        }
    }

    // Update time offset for animation
    timeOffset += timeSpeed * 16; // Approximate 60fps frame time

    // Continue animation
    animationId = requestAnimationFrame(render);
}

function resizeCanvas() {
    if (!canvas) return;
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
}

export function startCloudsAnimation() {
    canvas = document.getElementById('background-canvas');
    if (!canvas) return;

    ctx = canvas.getContext('2d');
    perlin = new PerlinNoise();

    resizeCanvas();
    window.addEventListener('resize', resizeCanvas);

    // Show canvas
    canvas.style.display = 'block';

    // Start animation
    if (!animationId) {
        render();
    }
}

export function stopCloudsAnimation() {
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
}

