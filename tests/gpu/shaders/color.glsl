#version 450
#ifdef VERTEX
layout(location = 0) in vec3 position;
layout(location = 1) in vec4 color;
layout(location = 0) out vec4 shade;
void main() {
    gl_Position = vec4(position, 1.0);
    shade = color;
}
#endif
#ifdef FRAGMENT
layout(location = 0) in vec4 shade;
layout(location = 0) out vec4 result;
void main() {
    result = shade;
}
#endif
