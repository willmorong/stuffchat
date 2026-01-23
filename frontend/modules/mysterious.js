/**
 * Mysterious theme animation - Smooth lava-lamp style with WebGPU
 */

let canvas = null;
let adapter = null;
let device = null;
let context = null;
let animationId = null;
let pipeline = null;
let uniformBuffer = null;
let bindGroup = null;
let timeOffset = 0;

// WGSL Shader Code
const shaderCode = `
struct Uniforms {
    time: f32,
    resolution: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

// 2D Random
fn random2(p: vec2<f32>) -> vec2<f32> {
    var p2 = vec2<f32>(
        dot(p, vec2<f32>(127.1, 311.7)),
        dot(p, vec2<f32>(269.5, 183.3))
    );
    return -1.0 + 2.0 * fract(sin(p2) * 43758.5453123);
}

// 2D Noise
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);

    let u = f * f * (3.0 - 2.0 * f);

    return mix(mix(dot(random2(i + vec2<f32>(0.0, 0.0)), f - vec2<f32>(0.0, 0.0)),
                   dot(random2(i + vec2<f32>(1.0, 0.0)), f - vec2<f32>(1.0, 0.0)), u.x),
               mix(dot(random2(i + vec2<f32>(0.0, 1.0)), f - vec2<f32>(0.0, 1.0)),
                   dot(random2(i + vec2<f32>(1.0, 1.0)), f - vec2<f32>(1.0, 1.0)), u.x), u.y);
}

fn lerpColor(c1: vec3<f32>, c2: vec3<f32>, t: f32) -> vec3<f32> {
    return mix(c1, c2, t);
}

fn getColorFromNoise(t: f32) -> vec3<f32> {
    // Colors mapped from 0-255 RGB to 0.0-1.0
    var c0 = vec3<f32>(0.0, 0.0, 0.0);           // Black
    var c1 = vec3<f32>(0.106, 0.149, 0.231);     // Mid blue
    var c2 = vec3<f32>(0.235, 0.196, 0.471);     // Purple accent
    var c3 = vec3<f32>(0.196, 0.118, 0.294);     // Darker purple
    var c4 = vec3<f32>(0.392, 0.588, 1.0);       // Vibrant blue
    var c5 = vec3<f32>(0.137, 0.471, 0.333);     // Dark teal

    let stops = 5.0;
    let scaledT = clamp(t, 0.0, 1.0) * stops;
    let index = i32(floor(scaledT));
    let frac = scaledT - f32(index);

    if (index == 0) { return lerpColor(c0, c1, frac); }
    if (index == 1) { return lerpColor(c1, c2, frac); }
    if (index == 2) { return lerpColor(c2, c3, frac); }
    if (index == 3) { return lerpColor(c3, c4, frac); }
    if (index == 4) { return lerpColor(c4, c5, frac); }
    return c5;
}

@vertex
fn vs_main(@builtin(vertex_index) vertexIndex: u32) -> @builtin(position) vec4<f32> {
    // Fullscreen triangle
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0)
    );
    return vec4<f32>(pos[vertexIndex], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) fragCoord: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = fragCoord.xy; // / uniforms.resolution.xy; // We can use pixel coords directly for noise scale

    // Noise parameters
    let baseScale = 0.003;
    let detailScale = 0.008;
    
    // Layer 1
    let n1 = noise(vec2<f32>(
        uv.x * baseScale + uniforms.time,
        uv.y * baseScale + uniforms.time * 0.6
    ));

    // Layer 2
    let n2 = noise(vec2<f32>(
        uv.x * baseScale * 0.7 - uniforms.time * 0.8,
        uv.y * baseScale * 0.7 + uniforms.time * 0.4
    ));

    // Layer 3
    let n3 = noise(vec2<f32>(
        uv.x * detailScale + uniforms.time * 1.5,
        uv.y * detailScale - uniforms.time * 0.3
    ));

    // Combine
    let combined = (n1 * 0.5 + n2 * 0.35 + n3 * 0.15);
    
    // Smoothstep
    let t_raw = (combined + 1.0) / 2.0;
    let t = t_raw * t_raw * (3.0 - 2.0 * t_raw);

    let color = getColorFromNoise(t);

    return vec4<f32>(color, 1.0);
}
`;

function resizeCanvas() {
    if (!canvas || !device) return;

    // Ensure canvas matches window size
    const width = window.innerWidth;
    const height = window.innerHeight;

    if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;

        // Reconfigure context on resize
        if (context) {
            context.configure({
                device: device,
                format: navigator.gpu.getPreferredCanvasFormat(),
                alphaMode: 'premultiplied',
            });
        }
    }
}

async function initWebGPU() {
    if (!navigator.gpu) {
        console.warn("WebGPU not supported on this browser.");
        return false;
    }

    adapter = await navigator.gpu.requestAdapter();
    if (!adapter) {
        console.warn("No appropriate WebGPU adapter found.");
        return false;
    }

    device = await adapter.requestDevice();

    context = canvas.getContext('webgpu');
    const presentationFormat = navigator.gpu.getPreferredCanvasFormat();

    context.configure({
        device: device,
        format: presentationFormat,
        alphaMode: 'premultiplied',
    });

    const module = device.createShaderModule({
        label: 'Mysterious Shader',
        code: shaderCode,
    });

    // Create render pipeline
    pipeline = device.createRenderPipeline({
        label: 'Mysterious Pipeline',
        layout: 'auto',
        vertex: {
            module: module,
            entryPoint: 'vs_main',
        },
        fragment: {
            module: module,
            entryPoint: 'fs_main',
            targets: [{ format: presentationFormat }],
        },
    });

    // Uniform Buffer: Time (f32) + Resolution (vec2<f32>) + padding to 16 bytes
    // f32 = 4 bytes
    // vec2 = 8 bytes
    // Structure: [time, res.x, res.y, padding] -> 16 bytes
    uniformBuffer = device.createBuffer({
        size: 16,
        usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
    });

    bindGroup = device.createBindGroup({
        layout: pipeline.getBindGroupLayout(0),
        entries: [
            { binding: 0, resource: { buffer: uniformBuffer } },
        ],
    });

    return true;
}

function render() {
    if (!device || !context || !pipeline || !canvas) return;

    // Update time
    // Slow down time
    const timeSpeed = 0.0001;
    timeOffset += timeSpeed * 16.0;

    // Write uniforms
    const uniforms = new Float32Array([
        timeOffset,
        canvas.width,
        canvas.height,
        0 // padding
    ]);
    device.queue.writeBuffer(uniformBuffer, 0, uniforms);

    const commandEncoder = device.createCommandEncoder();
    const textureView = context.getCurrentTexture().createView();

    const renderPassDescriptor = {
        colorAttachments: [{
            view: textureView,
            clearValue: { r: 0, g: 0, b: 0, a: 1 },
            loadOp: 'clear',
            storeOp: 'store',
        }],
    };

    const passEncoder = commandEncoder.beginRenderPass(renderPassDescriptor);
    passEncoder.setPipeline(pipeline);
    passEncoder.setBindGroup(0, bindGroup);
    passEncoder.draw(3); // Draw 3 vertices for full screen triangle
    passEncoder.end();

    device.queue.submit([commandEncoder.finish()]);

    animationId = requestAnimationFrame(render);
}

export async function startMysteriousAnimation() {
    canvas = document.getElementById('background-canvas');
    if (!canvas) return;

    // Clean up previous state if any
    stopMysteriousAnimation();

    // We must re-acquire canvas because stop() sets global canvas to null
    // (though in this specific flow, we just grabbed it. But stop() might null it if we passed it)
    // Actually stop() checks global canvas. We just set local canvas... wait.
    // In JS module scope, canvas is global.
    // So line 247 set the global canvas.
    // stopMysteriousAnimation() sets global canvas to null.
    // So we need to set it again.

    canvas = document.getElementById('background-canvas');
    if (!canvas) return;
    canvas.style.display = 'block';

    const success = await initWebGPU();

    // Check if stopped during async init
    // If stopped, canvas would be null (set by stopMysteriousAnimation)
    if (!canvas) {
        if (device) {
            device.destroy();
            device = null;
        }
        return;
    }

    if (!success) {
        console.error("Failed to initialize WebGPU for mysterious animation");
        return;
    }

    // Ensure context is configured
    if (!context) {
        context = canvas.getContext('webgpu');
        context.configure({
            device: device,
            format: navigator.gpu.getPreferredCanvasFormat(),
            alphaMode: 'premultiplied',
        });
    }

    resizeCanvas();
    window.addEventListener('resize', resizeCanvas);

    render();
}

export function stopMysteriousAnimation() {
    if (animationId) {
        cancelAnimationFrame(animationId);
        animationId = null;
    }

    window.removeEventListener('resize', resizeCanvas);

    if (canvas) {
        canvas.style.display = 'none';
        canvas = null;
    }

    // Destroy WebGPU resources
    if (device) {
        device.destroy();
        device = null;
    }
    context = null;
    adapter = null;
    pipeline = null;
    uniformBuffer = null;
    bindGroup = null;
}
