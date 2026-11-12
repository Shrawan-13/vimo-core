#version 300 es
// Contrast: a pivot of 0.5 in linear-ish space.
precision mediump float;

in vec2 v_texcoord;

uniform sampler2D u_texture;
uniform float u_amount;

out vec4 fragColor;

void main() {
    vec4 color = texture(u_texture, v_texcoord);
    vec3 rgb = (color.rgb - 0.5) * u_amount + 0.5;
    fragColor = vec4(clamp(rgb, 0.0, 1.0), color.a);
}
