#version 300 es
// Brightness: a uniform gain on the colour channels.
precision mediump float;

in vec2 v_texcoord;

uniform sampler2D u_texture;
uniform float u_amount;

out vec4 fragColor;

void main() {
    vec4 color = texture(u_texture, v_texcoord);
    fragColor = vec4(color.rgb * (1.0 + u_amount), color.a);
}
