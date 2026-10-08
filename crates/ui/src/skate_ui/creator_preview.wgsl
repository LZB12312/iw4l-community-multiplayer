struct PreviewUniform {
    clip_from_model: mat4x4<f32>,
    light: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> preview: PreviewUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var atlas: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var atlas_sampler: sampler;
struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
}
@vertex
fn vertex(input: Vertex) -> Output {
    var output: Output;
    output.position = preview.clip_from_model * vec4(input.position, 1.0);
    output.normal = input.normal;
    output.uv = input.uv;
    return output;
}
@fragment
fn fragment(input: Output) -> @location(0) vec4<f32> {
    let color = textureSample(atlas, atlas_sampler, input.uv);
    if color.a < 0.01 { discard; }
    let diffuse = max(dot(normalize(input.normal), normalize(preview.light.xyz)), 0.0);
    return vec4(color.rgb * (0.5 + 0.5 * diffuse), color.a);
}
