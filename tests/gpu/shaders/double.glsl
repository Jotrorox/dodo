#version 450
layout(local_size_x = 64) in;
layout(std430, binding = 0) readonly buffer Source { uint source[]; };
layout(std430, binding = 1) buffer Target { uint target[]; };
void main() {
    uint i = gl_GlobalInvocationID.x;
    target[i] = source[i] * 2u;
}
