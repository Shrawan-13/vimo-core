#version 300 es
// Sharpen: an unsharp mask over a 3x3 neighbourhood.
precision mediump float;

in vec2 v_texcoord;

uniform sampler2D u_texture;
uniform vec2 u_texel;
uniform float u_amount;

out vec4 fragColor;

void main() {
    vec4 centre = texture(u_texture, v_texcoord);
    vec3 blur = texture(u_texture, v_texcoord + vec2(u_texel.x, 0.0)).rgb
               + texture(u_texture, v_texcoord - vec2(u_texel.x, 0.0)).rgb
               + texture(u_texture, v_texcoord + vec2(0.0, u_texel.y)).rgb
               + texture(u_texture, v_texcoord - vec2(0.0, u_texel.y)).rgb;
    blur *= 0.25;
    vec3 sharpened = centre.rgb + (centre.rgb - blur) * u_amount;
    fragColor = vec4(clamp(sharpened, 0.0, 1.0), centre.a);
}
