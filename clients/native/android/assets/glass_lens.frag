#include <flutter/runtime_effect.glsl>

uniform vec2 imageSize;
uniform float density;
uniform float radius;
uniform float rim;
uniform float amount;
uniform float spread;
uniform float depth;
uniform sampler2D backdrop;
out vec4 fragColor;

vec2 texcoord(vec2 point) {
  vec2 safeSize = max(imageSize, vec2(1.0));
  vec2 uv = clamp(point / safeSize, vec2(0.0), vec2(1.0));
#ifdef IMPELLER_TARGET_OPENGLES
  uv.y = 1.0 - uv.y;
#endif
  return uv;
}

void main() {
  vec2 pos = FlutterFragCoord().xy;
  vec2 safeSize = max(imageSize, vec2(1.0));
  float safeDensity = max(density, 0.0001);
  float safeRim = max(rim, 0.0001);
  vec2 halfSize = safeSize * 0.5 / safeDensity;
  vec2 local = pos / safeDensity - halfSize;
  float r = min(radius, min(halfSize.x, halfSize.y));
  vec2 q = abs(local) - halfSize + r;
  float distance = min(length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r, 0.0);
  float t = clamp(1.0 + distance / safeRim, 0.0, 1.0);
  vec2 dq = abs(local) - halfSize + min(1.5 * r, min(halfSize.x, halfSize.y));
  vec2 outward = max(dq, 0.0);
  if (length(outward) < 0.0001) outward = dq.x >= dq.y ? vec2(1.0, 0.0) : vec2(0.0, 1.0);
  vec2 side = vec2(local.x < 0.0 ? -1.0 : 1.0, local.y < 0.0 ? -1.0 : 1.0);
  vec2 normal = normalize(outward) * side;
  normal += depth * local / max(length(local), 0.0001);
  vec2 offset = -normal / max(length(normal), 0.0001) * safeDensity * amount * (1.0 - sqrt(max(0.0, 1.0 - t*t)));
  vec4 green = texture(backdrop, texcoord(pos + offset));
  fragColor = vec4(texture(backdrop, texcoord(pos + offset * (1.0 - spread))).r,
                   green.g, texture(backdrop, texcoord(pos + offset * (1.0 + spread))).b, green.a);
}
