#version 300 es
// Tint: multiplies the picture by a colour.
precision mediump float;

in vec2 v_texcoord;

uniform sampler2D u_texture;
uniform vec4 u_color;

out vec4 fragColor;

void main() {
    vec4 color = texture(u_texture, v_texcoord);
    fragColor = vec4(color.rgb * u_color.rgb, color.a);
}
