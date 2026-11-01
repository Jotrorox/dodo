#version 450
#ifdef VULKAN
#define VERTEX_ID gl_VertexIndex
#else
#define VERTEX_ID gl_VertexID
#endif
#ifdef VERTEX
layout(location = 0) out vec2 uv;
void main() {
    vec2 corner = vec2((VERTEX_ID << 1) & 2, VERTEX_ID & 2);
    gl_Position = vec4(corner * 2.0 - 1.0, 0.0, 1.0);
    uv = vec2(corner.x, 1.0 - corner.y);
}
#endif
#ifdef FRAGMENT
layout(std140, binding = 0) uniform Color { vec4 color; };
layout(location = 0) out vec4 result;
void main() {
    result = color;
}
#endif
