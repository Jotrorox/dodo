#include <metal_stdlib>
using namespace metal;

struct Params {
    uint count;
    uint factor;
};

kernel void update(device uint *values [[buffer(0)]], constant Params &params [[buffer(1)]], uint i [[thread_position_in_grid]]) {
    if (i < params.count) {
        values[i] = values[i] * params.factor + i;
    }
}

kernel void double_values(device const uint *source [[buffer(0)]], device uint *target [[buffer(1)]], uint i [[thread_position_in_grid]]) {
    target[i] = source[i] * 2u;
}

kernel void fill(texture2d<uint, access::write> image [[texture(0)]], uint2 id [[thread_position_in_grid]]) {
    image.write(uint4(id.x + id.y * 100u, 0u, 0u, 0u), id);
}

struct ColorIn {
    float3 position [[attribute(0)]];
    float4 color [[attribute(1)]];
};

struct ColorOut {
    float4 position [[position]];
    float4 color;
};

vertex ColorOut color_vertex(ColorIn in [[stage_in]]) {
    ColorOut out;
    out.position = float4(in.position, 1.0);
    out.color = in.color;
    return out;
}

fragment float4 color_fragment(ColorOut in [[stage_in]]) {
    return in.color;
}

struct ScreenOut {
    float4 position [[position]];
    float2 uv;
};

vertex ScreenOut screen_vertex(uint id [[vertex_id]]) {
    float2 corner = float2((id << 1) & 2, id & 2);
    ScreenOut out;
    out.position = float4(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = float2(corner.x, 1.0 - corner.y);
    return out;
}

fragment float4 texture_fragment(ScreenOut in [[stage_in]], texture2d<float> image [[texture(0)]], sampler nearest [[sampler(0)]]) {
    return image.sample(nearest, in.uv);
}

fragment float4 constant_fragment(ScreenOut in [[stage_in]], constant float4 &color [[buffer(0)]]) {
    return color;
}
