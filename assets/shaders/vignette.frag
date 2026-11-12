#version 300 es
// Vignette: darkens the edges of the frame.
precision mediump float;

in vec2 v_texcoord;

uniform sampler2D u_texture;
uniform float u_amount;
uniform float u_radius;

out vec4 fragColor;

void main() {
    vec4 color = texture(u_texture, v_texcoord);
    vec2 centred = v_texcoord - 0.5;
    float distance = length(centred) * 1.41421356;
    float falloff = smoothstep(u_radius, u_radius * 0.25, distance);
    fragColor = vec4(color.rgb * mix(1.0 - u_amount, 1.0, falloff), color.a);
}
