#version 300 es
// Gaussian blur: a nine-tap binomial kernel.
//
// A separable blur would need two passes, which would double the render
// targets for a barely visible gain at this resolution; the 3x3 binomial
// kernel below is a good approximation in a single pass.
precision mediump float;

in vec2 v_texcoord;

uniform sampler2D u_texture;
uniform vec2 u_texel;
uniform float u_radius;

out vec4 fragColor;

void main() {
    vec2 step = u_texel * u_radius;
    vec4 centre = texture(u_texture, v_texcoord);
    vec4 sum = centre * 0.25;
    sum += texture(u_texture, v_texcoord + vec2(step.x, 0.0)) * 0.125;
    sum += texture(u_texture, v_texcoord - vec2(step.x, 0.0)) * 0.125;
    sum += texture(u_texture, v_texcoord + vec2(0.0, step.y)) * 0.125;
    sum += texture(u_texture, v_texcoord - vec2(0.0, step.y)) * 0.125;
    sum += texture(u_texture, v_texcoord + step) * 0.0625;
    sum += texture(u_texture, v_texcoord - step) * 0.0625;
    sum += texture(u_texture, v_texcoord + vec2(step.x, -step.y)) * 0.0625;
    sum += texture(u_texture, v_texcoord + vec2(-step.x, step.y)) * 0.0625;
    fragColor = sum;
}
