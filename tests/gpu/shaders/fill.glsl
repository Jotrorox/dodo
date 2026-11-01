#version 450
layout(local_size_x = 8, local_size_y = 8) in;
layout(r32ui, binding = 0) uniform writeonly uimage2D image;
void main() {
    uvec2 id = gl_GlobalInvocationID.xy;
    imageStore(image, ivec2(id), uvec4(id.x + id.y * 100u));
}
