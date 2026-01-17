/**
 * Rain theme animation - Rain on glass effect with WebGL
 * Creates a gray cloudy sky with raindrops sliding down the screen
 */

// Animation state
let canvas = null;
let gl = null;
let animationId = null;
let program = null;
let startTime = 0;

// Vertex shader - simple fullscreen quad
const vertexShaderSource = `
    attribute vec2 a_position;
    varying vec2 v_uv;
    
    void main() {
        v_uv = a_position * 0.5 + 0.5;
        gl_Position = vec4(a_position, 0.0, 1.0);
    }
`;

// Fragment shader - rain on glass effect
const fragmentShaderSource = `
    precision highp float;
    
    varying vec2 v_uv;
    uniform float u_time;
    uniform vec2 u_resolution;
    
    // Simplex noise functions for cloud generation
    vec3 mod289(vec3 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
    vec2 mod289(vec2 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
    vec3 permute(vec3 x) { return mod289(((x*34.0)+1.0)*x); }
    
    float snoise(vec2 v) {
        const vec4 C = vec4(0.211324865405187, 0.366025403784439,
                           -0.577350269189626, 0.024390243902439);
        vec2 i  = floor(v + dot(v, C.yy));
        vec2 x0 = v - i + dot(i, C.xx);
        vec2 i1 = (x0.x > x0.y) ? vec2(1.0, 0.0) : vec2(0.0, 1.0);
        vec4 x12 = x0.xyxy + C.xxzz;
        x12.xy -= i1;
        i = mod289(i);
        vec3 p = permute(permute(i.y + vec3(0.0, i1.y, 1.0)) + i.x + vec3(0.0, i1.x, 1.0));
        vec3 m = max(0.5 - vec3(dot(x0,x0), dot(x12.xy,x12.xy), dot(x12.zw,x12.zw)), 0.0);
        m = m*m; m = m*m;
        vec3 x = 2.0 * fract(p * C.www) - 1.0;
        vec3 h = abs(x) - 0.5;
        vec3 ox = floor(x + 0.5);
        vec3 a0 = x - ox;
        m *= 1.79284291400159 - 0.85373472095314 * (a0*a0 + h*h);
        vec3 g;
        g.x  = a0.x  * x0.x  + h.x  * x0.y;
        g.yz = a0.yz * x12.xz + h.yz * x12.yw;
        return 130.0 * dot(m, g);
    }
    
    // Fractal brownian motion for clouds
    float fbm(vec2 p) {
        float value = 0.0;
        float amplitude = 0.5;
        float frequency = 1.0;
        for (int i = 0; i < 5; i++) {
            value += amplitude * snoise(p * frequency);
            amplitude *= 0.5;
            frequency *= 2.0;
        }
        return value;
    }
    
    // Hash functions for raindrops
    float hash(float n) { return fract(sin(n) * 43758.5453123); }
    float hash2(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453123); }
    
    // Smooth raindrop shape
    float raindrop(vec2 uv, vec2 center, float size, float elongation) {
        vec2 d = uv - center;
        d.y /= elongation;
        float dist = length(d);
        
        // Teardrop shape - wider at bottom, pointed at top
        float shape = smoothstep(size, size * 0.3, dist);
        
        // Add slight refraction-like highlight
        float highlight = smoothstep(size * 0.8, size * 0.2, dist) * 0.5;
        
        return shape + highlight * 0.3;
    }
    
    // Trail behind raindrop
    float trail(vec2 uv, vec2 dropPos, float width, float length) {
        if (uv.y < dropPos.y || uv.y > dropPos.y + length) return 0.0;
        float xDist = abs(uv.x - dropPos.x);
        float yFactor = (uv.y - dropPos.y) / length;
        float trailWidth = width * (1.0 - yFactor * 0.5);
        return smoothstep(trailWidth, trailWidth * 0.3, xDist) * (1.0 - yFactor) * 0.4;
    }
    
    void main() {
        vec2 uv = v_uv;
        vec2 aspect = vec2(u_resolution.x / u_resolution.y, 1.0);
        
        // Gray cloudy sky background
        vec2 cloudUV = uv * 2.0;
        cloudUV.x += u_time * 0.02;
        
        float clouds = fbm(cloudUV * 1.5 + u_time * 0.01);
        clouds = clouds * 0.5 + 0.5;
        
        // Gray color palette for rainy sky
        vec3 darkGray = vec3(0.25, 0.27, 0.30);
        vec3 lightGray = vec3(0.55, 0.58, 0.62);
        vec3 midGray = vec3(0.40, 0.42, 0.45);
        
        vec3 skyColor = mix(darkGray, lightGray, clouds);
        
        // Add some depth variation
        float clouds2 = fbm(cloudUV * 0.8 - u_time * 0.015);
        skyColor = mix(skyColor, midGray, clouds2 * 0.3 + 0.2);
        
        // Rain drops layer
        float rainIntensity = 0.0;
        float displacement = 0.0;
        
        // Multiple layers of raindrops at different speeds/sizes
        for (int layer = 0; layer < 3; layer++) {
            float layerF = float(layer);
            float speed = 0.3 + layerF * 0.15;
            float size = 0.008 - layerF * 0.002;
            float density = 15.0 + layerF * 8.0;
            
            // Grid of potential raindrop positions
            vec2 gridUV = uv * vec2(density, density * 0.5);
            vec2 gridId = floor(gridUV);
            vec2 gridPos = fract(gridUV);
            
            // Check neighboring cells for drops
            for (int dx = -1; dx <= 1; dx++) {
                for (int dy = -1; dy <= 1; dy++) {
                    vec2 neighbor = gridId + vec2(float(dx), float(dy));
                    
                    // Random position within cell
                    float randX = hash2(neighbor + layerF * 100.0);
                    float randY = hash2(neighbor.yx + layerF * 100.0 + 50.0);
                    float randSpeed = hash2(neighbor + layerF * 200.0);
                    float randSize = hash2(neighbor + layerF * 300.0);
                    
                    // Only some cells have drops
                    if (hash2(neighbor + layerF * 400.0) > 0.7) continue;
                    
                    // Animated Y position (falling down)
                    float fallSpeed = speed * (0.8 + randSpeed * 0.4);
                    float yOffset = mod(u_time * fallSpeed + randY * 10.0, 2.0);
                    
                    vec2 dropCenter = vec2(
                        (neighbor.x + 0.3 + randX * 0.4) / density,
                        1.0 - yOffset + (neighbor.y + randY) / (density * 0.5)
                    );
                    
                    // Slight horizontal drift
                    dropCenter.x += sin(u_time * 0.5 + randX * 6.28) * 0.002;
                    
                    float dropSize = size * (0.6 + randSize * 0.8);
                    float elongation = 1.5 + randSpeed * 0.5;
                    
                    // Raindrop
                    float drop = raindrop(uv, dropCenter, dropSize, elongation);
                    rainIntensity += drop;
                    
                    // Trail
                    float trailEffect = trail(uv, dropCenter, dropSize * 0.4, dropSize * 8.0);
                    rainIntensity += trailEffect;
                    
                    // Displacement for refraction effect
                    vec2 toCenter = uv - dropCenter;
                    float distToCenter = length(toCenter);
                    if (distToCenter < dropSize * 2.0) {
                        displacement += (1.0 - distToCenter / (dropSize * 2.0)) * 0.02;
                    }
                }
            }
        }
        
        // Apply displacement to clouds (refraction through water)
        vec2 displacedUV = uv + vec2(displacement * 0.5, displacement);
        vec2 displacedCloudUV = displacedUV * 2.0 + u_time * 0.02;
        float displacedClouds = fbm(displacedCloudUV * 1.5 + u_time * 0.01);
        displacedClouds = displacedClouds * 0.5 + 0.5;
        
        vec3 refractedSky = mix(darkGray, lightGray, displacedClouds);
        float clouds2Displaced = fbm(displacedCloudUV * 0.8 - u_time * 0.015);
        refractedSky = mix(refractedSky, midGray, clouds2Displaced * 0.3 + 0.2);
        
        // Blend refracted sky where there are drops
        skyColor = mix(skyColor, refractedSky, clamp(displacement * 10.0, 0.0, 1.0));
        
        // Add rain drop highlights (brighter spots)
        rainIntensity = clamp(rainIntensity, 0.0, 1.0);
        vec3 dropColor = vec3(0.7, 0.75, 0.8);
        skyColor = mix(skyColor, dropColor, rainIntensity * 0.6);
        
        // Slight vignette for depth
        float vignette = 1.0 - length(uv - 0.5) * 0.3;
        skyColor *= vignette;
        
        gl_FragColor = vec4(skyColor, 1.0);
    }
`;

function createShader(gl, type, source) {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);

    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
        console.error('Shader compile error:', gl.getShaderInfoLog(shader));
        gl.deleteShader(shader);
        return null;
    }
    return shader;
}

function createProgram(gl, vertexShader, fragmentShader) {
    const program = gl.createProgram();
    gl.attachShader(program, vertexShader);
    gl.attachShader(program, fragmentShader);
    gl.linkProgram(program);

    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
        console.error('Program link error:', gl.getProgramInfoLog(program));
        gl.deleteProgram(program);
        return null;
    }
    return program;
}

function initWebGL() {
    const vertexShader = createShader(gl, gl.VERTEX_SHADER, vertexShaderSource);
    const fragmentShader = createShader(gl, gl.FRAGMENT_SHADER, fragmentShaderSource);

    if (!vertexShader || !fragmentShader) return false;

    program = createProgram(gl, vertexShader, fragmentShader);
    if (!program) return false;

    // Create fullscreen quad
    const positionBuffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([
        -1, -1,
        1, -1,
        -1, 1,
        -1, 1,
        1, -1,
        1, 1
    ]), gl.STATIC_DRAW);

    const positionLocation = gl.getAttribLocation(program, 'a_position');
    gl.enableVertexAttribArray(positionLocation);
    gl.vertexAttribPointer(positionLocation, 2, gl.FLOAT, false, 0, 0);

    return true;
}

function render() {
    if (!canvas || !gl || !program) return;

    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);

    gl.useProgram(program);

    // Set uniforms
    const timeLocation = gl.getUniformLocation(program, 'u_time');
    const resolutionLocation = gl.getUniformLocation(program, 'u_resolution');

    const elapsed = (performance.now() - startTime) / 1000;
    gl.uniform1f(timeLocation, elapsed);
    gl.uniform2f(resolutionLocation, canvas.width, canvas.height);

    // Draw
    gl.drawArrays(gl.TRIANGLES, 0, 6);

    animationId = requestAnimationFrame(render);
}

function resizeCanvas() {
    if (!canvas) return;
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
}

export function startRainAnimation() {
    canvas = document.getElementById('background-canvas');
    if (!canvas) return;

    gl = canvas.getContext('webgl') || canvas.getContext('experimental-webgl');
    if (!gl) {
        console.error('WebGL not supported');
        return;
    }

    resizeCanvas();
    window.addEventListener('resize', resizeCanvas);

    if (!initWebGL()) {
        console.error('Failed to initialize WebGL for rain effect');
        return;
    }

    startTime = performance.now();

    // Show canvas
    canvas.style.display = 'block';

    // Start animation
    if (!animationId) {
        render();
    }
}

export function stopRainAnimation() {
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
    gl = null;
    program = null;
}

