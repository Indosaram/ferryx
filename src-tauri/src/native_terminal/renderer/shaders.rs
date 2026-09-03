//! WGSL shader source code for background rects and textured glyph quads.

pub const BG_SHADER_SRC: &str = r#"
struct ScreenUniform {
    screen_size: vec2<f32>,
    _pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> screen: ScreenUniform;

struct VertexInput {
    @builtin(vertex_index) v_idx: u32,
    @location(0) rect: vec4<f32>,
    @location(1) color: vec4<f32>,
};
struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    var p = vec2<f32>(0.0, 0.0);
    switch in.v_idx {
        case 0u: { p = vec2<f32>(in.rect.x, in.rect.y); }
        case 1u: { p = vec2<f32>(in.rect.x + in.rect.z, in.rect.y); }
        case 2u: { p = vec2<f32>(in.rect.x, in.rect.y + in.rect.w); }
        case 3u: { p = vec2<f32>(in.rect.x, in.rect.y + in.rect.w); }
        case 4u: { p = vec2<f32>(in.rect.x + in.rect.z, in.rect.y); }
        case 5u: { p = vec2<f32>(in.rect.x + in.rect.z, in.rect.y + in.rect.w); }
        default: {}
    }
    let ndc_x = (p.x / screen.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (p.y / screen.screen_size.y) * 2.0;
    out.clip_pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

pub const GLYPH_SHADER_SRC: &str = r#"
struct ScreenUniform {
    screen_size: vec2<f32>,
    _pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> screen: ScreenUniform;
@group(1) @binding(0) var mask_tex: texture_2d<f32>;
@group(1) @binding(1) var color_tex: texture_2d<f32>;
@group(1) @binding(2) var atlas_sampler: sampler;

struct VertexInput {
    @builtin(vertex_index) v_idx: u32,
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) flags: vec4<f32>,
};
struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) is_color: f32,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    var p = vec2<f32>(0.0, 0.0);
    var u = vec2<f32>(0.0, 0.0);
    switch in.v_idx {
        case 0u: { p = vec2<f32>(in.rect.x, in.rect.y); u = vec2<f32>(in.uv.x, in.uv.y); }
        case 1u: { p = vec2<f32>(in.rect.x + in.rect.z, in.rect.y); u = vec2<f32>(in.uv.z, in.uv.y); }
        case 2u: { p = vec2<f32>(in.rect.x, in.rect.y + in.rect.w); u = vec2<f32>(in.uv.x, in.uv.w); }
        case 3u: { p = vec2<f32>(in.rect.x, in.rect.y + in.rect.w); u = vec2<f32>(in.uv.x, in.uv.w); }
        case 4u: { p = vec2<f32>(in.rect.x + in.rect.z, in.rect.y); u = vec2<f32>(in.uv.z, in.uv.y); }
        case 5u: { p = vec2<f32>(in.rect.x + in.rect.z, in.rect.y + in.rect.w); u = vec2<f32>(in.uv.z, in.uv.w); }
        default: {}
    }
    let ndc_x = (p.x / screen.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (p.y / screen.screen_size.y) * 2.0;
    out.clip_pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = u;
    out.color = in.color;
    out.is_color = in.flags.x;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if in.is_color > 0.5 {
        let c = textureSample(color_tex, atlas_sampler, in.uv);
        if c.a > 0.001 {
            return vec4<f32>(c.rgb / c.a, c.a);
        }
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    } else {
        let cov = textureSample(mask_tex, atlas_sampler, in.uv);
        // The swapchain is UNORM, so the GPU blends coverage directly on sRGB-encoded
        // channels. A linear ramp therefore lands edge pixels far darker than their
        // photometric share (0.5 coverage -> 21% light), thinning every stroke. The
        // exponent pre-compensates that encoding.
        let coverage = pow(cov.a, 0.7142857) * in.color.a;
        return vec4<f32>(in.color.rgb, coverage);
    }
}
"#;
