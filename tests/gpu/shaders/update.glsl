#version 450
layout(local_size_x = 64) in;
layout(std430, binding = 0) buffer Data { uint values[]; };
layout(std140, binding = 1) uniform Params { uint count; uint factor; };
void main() {
    uint i = gl_GlobalInvocationID.x;
    if (i < count) {
        values[i] = values[i] * factor + i;
    }
}
