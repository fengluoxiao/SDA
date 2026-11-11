import * as THREE from "three";
import type { CardboardConfig } from "./cardboard-config";

export type DrawScene = (scene: THREE.Object3D, camera: THREE.Camera) => void;
// Capture before Fiber installs its render-and-present wrapper. Offscreen eyes
// must never present Expo's drawable; only the completed warped pair presents.
const rawDraws = new WeakMap<THREE.WebGLRenderer, DrawScene>();
export function createSpaceRenderer(parameters: THREE.WebGLRendererParameters) {
  const renderer = new THREE.WebGLRenderer({ ...parameters, antialias: false, alpha: false });
  rawDraws.set(renderer, renderer.render.bind(renderer));
  return renderer;
}
export function getSpaceRawDraw(renderer: THREE.WebGLRenderer): DrawScene {
  const draw = rawDraws.get(renderer);
  if (!draw) throw new Error("空间渲染器未保留原始绘制入口");
  return draw;
}
const vertexShader = `varying vec2 vUv;
void main() { vUv = uv; gl_Position = vec4(position.xy, 0.0, 1.0); }`;
const fragmentShader = `precision highp float;
uniform sampler2D leftEye;
uniform sampler2D rightEye;
uniform float eyeAspect;
uniform float lensRatio;
uniform float tangent;
uniform vec2 distortion;
uniform float divider;
uniform bool calibration;
varying vec2 vUv;
void main() {
  bool left = vUv.x < 0.5;
  vec2 local = vec2(left ? vUv.x * 2.0 : (vUv.x - 0.5) * 2.0, vUv.y);
  vec2 center = vec2(left ? 1.0 - lensRatio : lensRatio, 0.5);
  vec2 ray = (local - center) * vec2(eyeAspect, 1.0) * (2.0 * tangent);
  float r2 = dot(ray, ray);
  vec2 sampleUv = vec2(0.5) + ray * (1.0 + distortion.x * r2 + distortion.y * r2 * r2) / (vec2(eyeAspect, 1.0) * (2.0 * tangent));
  if (abs(vUv.x - 0.5) < divider || min(sampleUv.x, sampleUv.y) < 0.0 || max(sampleUv.x, sampleUv.y) > 1.0) {
    gl_FragColor = vec4(0.0, 0.0, 0.0, 1.0);
  } else {
    if (calibration) {
      vec2 line = abs(fract(sampleUv * 10.0 + 0.5) - 0.5);
      float grid = 1.0 - smoothstep(0.006, 0.016, min(line.x, line.y));
      vec3 color = mix(vec3(0.015), vec3(0.5), grid);
      if (abs(sampleUv.y - 0.5) < 0.003) color = vec3(0.2, 0.9, 0.45);
      if (abs(sampleUv.x - 0.5) < 0.003) color = vec3(0.9, 0.35, 0.2);
      gl_FragColor = vec4(color, 1.0);
    } else gl_FragColor = left ? texture2D(leftEye, sampleUv) : texture2D(rightEye, sampleUv);
  }
}`;

/** Bounded GPU eye surfaces, reused across frames and explicitly disposed. */
export class CardboardRenderer {
  readonly stereo = new THREE.StereoCamera();
  readonly left = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: true, stencilBuffer: false });
  readonly right = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: true, stencilBuffer: false });
  readonly output = new THREE.Scene();
  readonly outputCamera = new THREE.Camera();
  readonly geometry = new THREE.PlaneGeometry(2, 2);
  readonly material = new THREE.ShaderMaterial({ vertexShader, fragmentShader, depthTest: false, depthWrite: false, toneMapped: false,
    uniforms: { leftEye: { value: this.left.texture }, rightEye: { value: this.right.texture }, eyeAspect: { value: 1 },
      lensRatio: { value: .5 }, tangent: { value: 1 }, distortion: { value: new THREE.Vector2() }, divider: { value: .001 }, calibration: { value: false } } });
  private size = new THREE.Vector2();
  constructor() {
    this.stereo.aspect = .5;
    this.left.texture.colorSpace = THREE.SRGBColorSpace;
    this.right.texture.colorSpace = THREE.SRGBColorSpace;
    const quad = new THREE.Mesh(this.geometry, this.material); quad.frustumCulled = false;
    this.output.add(quad);
  }
  render(renderer: THREE.WebGLRenderer, draw: DrawScene, scene: THREE.Object3D, camera: THREE.PerspectiveCamera, config: CardboardConfig, calibration = false) {
    renderer.getDrawingBufferSize(this.size);
    const width = Math.max(2, this.size.x), height = Math.max(1, this.size.y);
    const scale = Math.min(1, 1024 / Math.max(width / 2, height));
    const eyeWidth = Math.max(1, Math.floor(width / 2 * scale)), eyeHeight = Math.max(1, Math.floor(height * scale));
    if (this.left.width !== eyeWidth || this.left.height !== eyeHeight) {
      this.left.setSize(eyeWidth, eyeHeight); this.right.setSize(eyeWidth, eyeHeight);
    }
    this.stereo.eyeSep = config.eyeDistanceMm / 1000;
    camera.updateMatrixWorld(); this.stereo.update(camera);
    const uniforms = this.material.uniforms;
    uniforms.calibration!.value = calibration;
    uniforms.eyeAspect!.value = width / (2 * height);
    uniforms.lensRatio!.value = config.lensDistanceMm / config.screenWidthMm;
    uniforms.tangent!.value = Math.tan(config.fov * Math.PI / 360);
    uniforms.distortion!.value.set(config.k1, config.k2); uniforms.divider!.value = 1 / width;
    const target = renderer.getRenderTarget();
    const scissor = renderer.getScissorTest();
    const autoClear = renderer.autoClear;
    const xr = renderer.xr.enabled;
    try {
      renderer.xr.enabled = false; renderer.autoClear = true; renderer.setScissorTest(false);
      renderer.setRenderTarget(this.left); draw(scene, this.stereo.cameraL);
      renderer.setRenderTarget(this.right); draw(scene, this.stereo.cameraR);
      renderer.setRenderTarget(target); draw(this.output, this.outputCamera);
    } finally {
      renderer.setRenderTarget(target); renderer.setScissorTest(scissor); renderer.autoClear = autoClear; renderer.xr.enabled = xr;
    }
  }
  dispose() { this.left.dispose(); this.right.dispose(); this.geometry.dispose(); this.material.dispose(); this.output.clear(); }
}
