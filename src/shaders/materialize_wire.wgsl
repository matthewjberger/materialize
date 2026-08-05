#import nightshade_renderer::material_data::reveal_noise

struct Uniforms {
    view_projection: mat4x4<f32>,
    model: mat4x4<f32>,
    camera_position: vec4<f32>,
    color: vec4<f32>,
    noise: vec4<f32>,
    wire_front: f32,
    solid_front: f32,
    band: f32,
    thickness: f32,
    glow_strength: f32,
    padding_a: f32,
    padding_b: f32,
    padding_c: f32,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) other: vec3<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) side: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let world = (uniforms.model * vec4<f32>(input.position, 1.0)).xyz;
    let world_other = (uniforms.model * vec4<f32>(input.other, 1.0)).xyz;
    let world_normal = normalize((uniforms.model * vec4<f32>(input.normal, 0.0)).xyz);

    let along = world_other - world;
    let length_along = length(along);
    let direction = select(vec3<f32>(1.0, 0.0, 0.0), along / length_along, length_along > 1.0e-6);

    let to_camera = normalize(uniforms.camera_position.xyz - world);
    var across = cross(direction, to_camera);
    let width = length(across);
    across = select(
        normalize(cross(direction, world_normal)),
        across / width,
        width > 1.0e-5
    );

    let offset = across * (input.side * uniforms.thickness * 0.5) + world_normal * 0.002;
    let expanded = world + offset;

    var output: VertexOutput;
    output.clip_position = uniforms.view_projection * vec4<f32>(expanded, 1.0);
    output.world_position = expanded;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let band = max(uniforms.band, 1.0e-4);
    let wobble = reveal_noise(input.world_position * uniforms.noise.xyz) * uniforms.noise.w;
    let wire_front = uniforms.wire_front + wobble;
    let solid_front = uniforms.solid_front + wobble;
    let height = input.world_position.y;

    let reveal = 1.0 - smoothstep(wire_front - band, wire_front, height);
    let takeover = smoothstep(solid_front - band, solid_front + band, height);
    let alpha = reveal * takeover * uniforms.color.a;
    if alpha <= 0.003 {
        discard;
    }

    let edge = 1.0 - clamp(abs(height - wire_front) / (band * 2.0), 0.0, 1.0);
    let color = uniforms.color.rgb * (1.0 + uniforms.glow_strength * edge * edge);
    return vec4<f32>(color * alpha, alpha);
}
