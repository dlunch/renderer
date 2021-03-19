#version 450

layout(location = 0) in vec2 Position;
layout(location = 1) in vec2 TexCoord;
layout(location = 0) out vec2 FragTexCoord;

void main() {
       gl_Position = vec4(Position.xy, 0.0, 1.0);
       FragTexCoord = TexCoord;
}