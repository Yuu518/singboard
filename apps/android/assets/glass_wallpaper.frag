#include <flutter/runtime_effect.glsl>

uniform vec2 uSize;
uniform vec2 uCoverOrigin;
uniform vec2 uCoverSize;
uniform float uRadius;
uniform float uRim;
uniform float uAmount;
uniform float uSpread;
uniform sampler2D uWallpaper;
out vec4 fragColor;

void main() {
  vec2 pos = FlutterFragCoord().xy;
  vec2 halfSize = max(uSize * 0.5, vec2(1.0));
  vec2 local = pos - halfSize;
  float r = min(uRadius, min(halfSize.x, halfSize.y));
  vec2 q = abs(local) - halfSize + r;
  float dist = min(length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r, 0.0);
  float rim = max(uRim, 1.0);
  float t = clamp(1.0 + dist / rim, 0.0, 1.0);
  vec2 dq = abs(local) - halfSize + min(1.5 * r, min(halfSize.x, halfSize.y));
  vec2 outward = max(dq, 0.0);
  if (length(outward) < 0.0001) {
    outward = dq.x >= dq.y ? vec2(1.0, 0.0) : vec2(0.0, 1.0);
  }
  vec2 side = vec2(local.x < 0.0 ? -1.0 : 1.0, local.y < 0.0 ? -1.0 : 1.0);
  vec2 normal = normalize(outward) * side;
  vec2 offset = normal * (uAmount * t * t);
  vec2 safeCover = max(uCoverSize, vec2(1.0));
  vec2 uvG = clamp((pos + offset - uCoverOrigin) / safeCover, vec2(0.0), vec2(1.0));
  vec4 green = texture(uWallpaper, uvG);
  if (uSpread > 0.001 && t > 0.01) {
    vec2 uvR = clamp((pos + offset * (1.0 - uSpread) - uCoverOrigin) / safeCover, vec2(0.0), vec2(1.0));
    vec2 uvB = clamp((pos + offset * (1.0 + uSpread) - uCoverOrigin) / safeCover, vec2(0.0), vec2(1.0));
    fragColor = vec4(texture(uWallpaper, uvR).r, green.g, texture(uWallpaper, uvB).b, green.a);
  } else {
    fragColor = green;
  }
}
