#version 300 es
// Saturation: mixes between the luma and the colour.
precision mediump float;

in vec2 v_texcoord;

uniform sampler2D u_texture;
uniform float u_amount;

out vec4 fragColor;

void main() {
    vec4 color = texture(u_texture, v_texcoord);
    float luma = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
    vec3 rgb = mix(vec3(luma), color.rgb, u_amount);
    fragColor = vec4(clamp(rgb, 0.0, 1.0), color.a);
}
