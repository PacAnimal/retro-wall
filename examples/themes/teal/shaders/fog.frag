// A low fog that drifts to the left, thicker as readings.x (the CPU, 0 to 1) rises. GLSL 1.20.
// Given: time, resolution, area, readings, uv (0 to 1 over the area). Premultiplied output.
float wave(vec2 p) {
    return sin(p.x * 7.0 + time * 0.3) * 0.5 + sin(p.x * 13.0 - p.y * 5.0 + time * 0.17) * 0.3;
}

void main() {
    float drift = 0.5 + 0.5 * wave(uv + vec2(time * 0.02, 0.0));
    float a = (0.08 + 0.18 * readings.x) * drift * smoothstep(0.0, 0.6, uv.y) * (1.0 - uv.y * 0.3);
    gl_FragColor = vec4(vec3(0.75, 0.95, 0.9) * a, a);
}
