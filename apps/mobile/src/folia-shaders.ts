import { vertexShaderSource } from "../../../vendor/folia/paper-shaders/dist/vertex-shader.js";
import { meshGradientFragmentShader } from "../../../vendor/folia/paper-shaders/dist/shaders/mesh-gradient.js";
import { ditheringFragmentShader } from "../../../vendor/folia/paper-shaders/dist/shaders/dithering.js";
export { meshGradientFragmentShader, ditheringFragmentShader, vertexShaderSource };
/** Syntax-only ES300 -> ES100 bridge for Three/Expo. Upstream algorithms are untouched. */
export function foliaVertexShader(): string {
  let value = vertexShaderSource.replace(/^#version 300 es\s*/, "")
    .replace(/layout\(location = 0\) in vec4 a_position;/, "attribute vec3 position;")
    .replace(/gl_Position = a_position;/, "gl_Position = vec4(position, 1.0);")
    .replace(/^out /gm, "varying ");
  value = "uniform float u_sdaScale;\n" + value;
  const end = value.lastIndexOf("}");
  return value.slice(0,end) + "gl_Position.xy *= u_sdaScale;\n" + value.slice(end);
}
export function foliaFragmentShader(source: string, opacity = false, filter = false): string {
  let value = source.replace(/^#version 300 es\s*/, "").replace(/^in /gm, "varying ")
    .replace(/^out vec4 fragColor;\s*/m, opacity ? "uniform float u_sdaOpacity;\n" : "").replace(/\bfragColor\b/g, "gl_FragColor");
  // ES100 lacks initialized constant arrays and switch statements. Expand the
  // original Bayer lookup into constant-index branches, preserving every value.
  value = value.replace(/const int (bayer\dx\d)\[\d+\] = int\[\d+\]\(([\s\S]*?)\);/g, (_, name: string, list: string) => {
    const values = list.split(",").map((v: string) => v.trim()).filter(Boolean);
    return "int " + name + "At(int index){" + values.map((v: string, i: number) => "if(index==" + i + ")return " + v + ";").join("") + "return 0;}";
  });
  value = value.replace(/(bayer\dx\d)\[index\]/g, "$1At(index)");
  value = value.replace(/switch \(type\) \{[\s\S]*?\n  \}/, original => {
    if (!original.includes("case 1:") || !original.includes("default")) throw new Error("Folia upstream dithering dispatch changed");
    return "if(type==1){dithering=step(hash21(ditheringNoiseUV),shape);}else if(type==2){dithering=getBayerValue(pxSizeUV,2);}else if(type==3){dithering=getBayerValue(pxSizeUV,4);}else{dithering=getBayerValue(pxSizeUV,8);}";
  });
  if (filter) {
    value = value.replace("precision mediump float;", "precision mediump float;uniform float u_sdaSaturation,u_sdaBrightness;");
    value = value.replace(/gl_FragColor = vec4\(color, opacity\);/, "vec3 gray=vec3(dot(color,vec3(.2126,.7152,.0722)));gl_FragColor=vec4(mix(gray,color,u_sdaSaturation)*u_sdaBrightness,opacity);");
  }
  if (opacity) value = value.replace(/gl_FragColor = vec4\(color, opacity\);/, "gl_FragColor = vec4(color, opacity * u_sdaOpacity);");
  if (/#version|\bswitch\b|const int bayer.*\[/.test(value)) throw new Error("Unsupported unadapted Folia GLSL syntax");
  return value;
}
export const FOLIA_VERTEX = foliaVertexShader();
export const FOLIA_MESH_FRAGMENT = foliaFragmentShader(meshGradientFragmentShader, false, true);
export const FOLIA_DITHER_FRAGMENT = foliaFragmentShader(ditheringFragmentShader, true);
