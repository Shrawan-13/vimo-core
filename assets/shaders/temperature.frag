#version 300 es
// Temperature: warms or cools the picture.
precision mediump float;

in vec2 v_texcoord;

uniform sampler2D u_texture;
uniform float u_amount;

out vec4 fragColor;

void main() {
    vec4 color = texture(u_texture, v_texcoord);
    // Positive values warm (more red, less blue), negative values cool.
    vec3 rgb = color.rgb + vec3(u_amount * 0.08, u_amount * 0.01, -u_amount * 0.08);
    fragColor = vec4(clamp(rgb, 0.0, 1.0), color.a);
}
