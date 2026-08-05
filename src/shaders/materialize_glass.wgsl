#import nightshade_renderer::material_data::reveal_noise

struct Uniforms {
    view_projection: mat4x4<f32>,
    model: mat4x4<f32>,
    camera_position: vec4<f32>,
    model_center: vec4<f32>,
    color: vec4<f32>,
    glow: vec4<f32>,
    fly: vec4<f32>,
    noise: vec4<f32>,
    glass_front: f32,
    solid_front: f32,
    band: f32,
    time: f32,
    center_distance: f32,
    normal_distance: f32,
    jitter: f32,
    fade_portion: f32,
    cool_span: f32,
    tumble: f32,
    padding_a: f32,
    padding_b: f32,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@group(1) @binding(0) var irradiance_map: texture_cube<f32>;
@group(1) @binding(1) var prefiltered_map: texture_cube<f32>;
@group(1) @binding(2) var brdf_lut: texture_2d<f32>;
@group(1) @binding(3) var environment_sampler: sampler;
@group(1) @binding(4) var lut_sampler: sampler;

const MAX_REFLECTION_LOD: f32 = 4.0;
const GLASS_ROUGHNESS: f32 = 0.06;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) centroid: vec3<f32>,
    @location(3) seed: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) flight: f32,
    @location(3) seated_for: f32,
};

fn rotate_about(axis: vec3<f32>, value: vec3<f32>, cosine: f32, sine: f32) -> vec3<f32> {
    return value * cosine + cross(axis, value) * sine + axis * dot(axis, value) * (1.0 - cosine);
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let centroid_height = (uniforms.model * vec4<f32>(input.centroid, 1.0)).y;

    let raw = (uniforms.glass_front - centroid_height) / max(uniforms.band, 1.0e-4);
    let flight = clamp(raw, 0.0, 1.0);

    let eased = flight * flight * flight * (flight * (flight * 6.0 - 15.0) + 10.0);
    let scatter = 1.0 - eased;

    let relative = input.position - input.centroid;
    let axis = normalize(
        vec3<f32>(sin(input.seed * 19.3), cos(input.seed * 35.7), sin(input.seed * 45.9))
        + vec3<f32>(0.0, 0.001, 0.0)
    );
    let wobble = sin(uniforms.time * (1.0 + fract(input.seed) * 1.5) + input.seed * 40.0) * 0.4;
    let angle = scatter * (uniforms.tumble * (fract(input.seed * 7.61) - 0.5) * 8.0 + wobble);
    let cosine = cos(angle);
    let sine = sin(angle);
    let relative_rotated = rotate_about(axis, relative, cosine, sine);
    let normal_rotated = rotate_about(axis, input.normal, cosine, sine);

    let seated = (uniforms.model * vec4<f32>(input.centroid + relative_rotated, 1.0)).xyz;

    let bias = normalize(uniforms.fly.xyz + vec3<f32>(0.0, 1.0e-4, 0.0));
    var radial = seated - uniforms.model_center.xyz;
    let radial_length = length(radial);
    radial = select(vec3<f32>(0.0, 1.0, 0.0), radial / radial_length, radial_length > 1.0e-5);
    let world_normal = normalize((uniforms.model * vec4<f32>(normal_rotated, 0.0)).xyz);
    let face_normal = normalize((uniforms.model * vec4<f32>(input.normal, 0.0)).xyz);
    let jitter_direction = normalize(vec3<f32>(
        sin(input.seed * 61.0),
        sin(input.seed * 47.0 + 2.0),
        cos(input.seed * 53.0)
    ));
    let offset = bias * uniforms.fly.w
        + radial * uniforms.center_distance
        + face_normal * uniforms.normal_distance
        + jitter_direction * (uniforms.jitter * (0.5 + fract(input.seed * 9.19)));

    var output: VertexOutput;
    let world_position = seated + offset * scatter;
    output.clip_position = uniforms.view_projection * vec4<f32>(world_position, 1.0);
    output.world_position = world_position;
    output.world_normal = world_normal;
    output.flight = flight;
    output.seated_for = max(raw - 1.0, 0.0);
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let appear = smoothstep(0.0, max(uniforms.fade_portion, 1.0e-3), input.flight);
    let wobble = reveal_noise(input.world_position * uniforms.noise.xyz) * uniforms.noise.w;
    let front = uniforms.solid_front + wobble;
    let takeover = smoothstep(front - 0.01, front, input.world_position.y);

    var normal = normalize(input.world_normal);
    let view = normalize(uniforms.camera_position.xyz - input.world_position);
    if dot(normal, view) < 0.0 {
        normal = -normal;
    }
    let normal_dot_view = max(dot(normal, view), 1.0e-4);
    let fresnel = pow(1.0 - normal_dot_view, 3.0);

    var alpha = mix(uniforms.color.a, 0.9, fresnel) * appear * takeover;
    if alpha <= 0.003 {
        discard;
    }

    let reflection = reflect(-view, normal);
    let prefiltered = textureSampleLevel(
        prefiltered_map,
        environment_sampler,
        reflection,
        GLASS_ROUGHNESS * MAX_REFLECTION_LOD
    ).rgb;
    let split_sum = textureSampleLevel(
        brdf_lut,
        lut_sampler,
        vec2<f32>(normal_dot_view, GLASS_ROUGHNESS),
        0.0
    ).rg;
    let irradiance = textureSampleLevel(irradiance_map, environment_sampler, normal, 0.0).rgb;

    let reflectance = vec3<f32>(0.04);
    let fresnel_response = reflectance + (max(vec3<f32>(1.0 - GLASS_ROUGHNESS), reflectance) - reflectance) * fresnel;
    let specular = prefiltered * (fresnel_response * split_sum.x + split_sum.y);
    let diffuse = irradiance * uniforms.color.rgb * (1.0 - fresnel);

    let ignite = smoothstep(0.6, 1.0, input.flight);
    let cool = 1.0 - smoothstep(0.0, max(uniforms.cool_span, 1.0e-3), input.seated_for);
    let emissive = uniforms.glow.rgb * (uniforms.glow.w * ignite * cool * (0.35 + 0.65 * fresnel));

    let color = diffuse * 0.35 + specular + emissive;
    return vec4<f32>(color * alpha, alpha);
}
