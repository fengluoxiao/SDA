import { defaultObjectSizing, ShaderFitOptions } from "../../../vendor/folia/paper-shaders/dist/shader-sizing.js";
import * as THREE from "three";
import { FOLIA_VERTEX, FOLIA_MESH_FRAGMENT } from "./folia-shaders";
import { FOLIA_SPATIAL_TUNING, foliaColors } from "./folia-runtime";
import { MAX_SHADER_PIXELS } from "../../../vendor/folia/latent.generated";
import type { AiSceneTheme } from "./ai-scene";
import type { DrawScene } from "./cardboard-renderer";
const rgba = (hex: string) => new THREE.Vector4(parseInt(hex.slice(1,3),16)/255, parseInt(hex.slice(3,5),16)/255, parseInt(hex.slice(5,7),16)/255, 1);
const sizing = (defaults = defaultObjectSizing) => ({ u_resolution: { value: new THREE.Vector2(1,1) }, u_pixelRatio: { value: 1 }, u_imageAspectRatio: { value: 1 }, u_originX: { value: defaults.originX }, u_originY: { value: defaults.originY }, u_worldWidth: { value: defaults.worldWidth }, u_worldHeight: { value: defaults.worldHeight }, u_fit: { value: ShaderFitOptions[defaults.fit] as number }, u_scale: { value: defaults.scale }, u_rotation: { value: defaults.rotation }, u_offsetX: { value: defaults.offsetX }, u_offsetY: { value: defaults.offsetY }, u_time: { value: 0 }, u_sdaScale: { value: 1 } });
/** Original Folia/Paper shaders in a bounded offscreen surface. Main/VR presents once. */
export class FoliaCompositor {
  readonly target = new THREE.WebGLRenderTarget(1,1,{ depthBuffer: false, stencilBuffer: false });
  readonly meshUniforms = { ...sizing(), u_colors: { value: Array.from({ length: 10 }, () => new THREE.Vector4()) }, u_colorsCount: { value: 6 }, u_distortion: { value: FOLIA_SPATIAL_TUNING.distortion as number }, u_swirl: { value: FOLIA_SPATIAL_TUNING.swirl as number }, u_grainMixer: { value: 0 }, u_grainOverlay: { value: 0 }, u_sdaSaturation: { value: 1 }, u_sdaBrightness: { value: 1 } };
  readonly scene = new THREE.Scene(); readonly camera = new THREE.Camera();
  private geometry = new THREE.PlaneGeometry(2,2);
  private meshMaterial = new THREE.RawShaderMaterial({ uniforms: this.meshUniforms, vertexShader: FOLIA_VERTEX, fragmentShader: FOLIA_MESH_FRAGMENT, depthTest: false, depthWrite: false });
  private viewport = new THREE.Vector4(); private scissor = new THREE.Vector4();
  private colorTargets: THREE.Vector4[] = []; private initialized = false;
  constructor() {
    const mesh = new THREE.Mesh(this.geometry, this.meshMaterial);
    mesh.name = "folia-soft-gradient"; mesh.frustumCulled = false; this.scene.add(mesh);
  }
  setTheme(theme: AiSceneTheme) {
    const colors = foliaColors(theme); this.colorTargets = colors.mesh.map(rgba);
    if (!this.initialized) { this.interpolate(1); this.initialized = true; }
  }
  interpolate(alpha: number) {
    for (let i=0;i<10;i++) { const target = this.colorTargets[i] || this.colorTargets[0]; if (target) this.meshUniforms.u_colors.value[i]!.lerp(target,alpha); }
  }
  draw(renderer: THREE.WebGLRenderer, raw: DrawScene, width: number, height: number, pixelRatio: number) {
    if (width<=0 || height<=0) return;
    const scale=Math.min(pixelRatio,Math.sqrt(MAX_SHADER_PIXELS/(width*height)));
    const w=Math.max(1,Math.round(width*scale)), h=Math.max(1,Math.round(height*scale));
    if(this.target.width!==w||this.target.height!==h)this.target.setSize(w,h);
    for(const u of [this.meshUniforms]){u.u_resolution.value.set(w,h);u.u_pixelRatio.value=scale;}
    const target=renderer.getRenderTarget(), autoClear=renderer.autoClear, scissorTest=renderer.getScissorTest();
    renderer.getViewport(this.viewport);renderer.getScissor(this.scissor);
    // Expo GL rejects FRAMEBUFFER_BINDING (the DRAW_FRAMEBUFFER_BINDING alias).
    // Three owns the target handles and restores the native drawable via null.
    try { renderer.setRenderTarget(this.target);renderer.setViewport(0,0,w,h);renderer.setScissorTest(false);renderer.autoClear=true;raw(this.scene,this.camera); }
    finally {renderer.setRenderTarget(target);renderer.setViewport(this.viewport);renderer.setScissor(this.scissor);renderer.setScissorTest(scissorTest);renderer.autoClear=autoClear;}
  }
  dispose() {this.target.dispose();this.geometry.dispose();this.meshMaterial.dispose();}
}
