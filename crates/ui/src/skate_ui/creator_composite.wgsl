#import bevy_ui::ui_vertex_output::UiVertexOutput
#import bevy_render::color_operations::linear_to_srgb
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;
@fragment
fn fragment(input: UiVertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(image, image_sampler, input.uv);
#ifdef ENCODE_SRGB
    return vec4(linear_to_srgb(color.rgb), color.a);
#else
    return color;
#endif
}
