import { GLView } from "expo-gl";
import { Asset } from "expo-asset";
const cache = new Map<string, string>();
// Read a small bottom-band sample from the actual artwork, not its filename.
export async function artworkColor(uri: string): Promise<string> {
  const cached = cache.get(uri); if (cached) return cached;
  const asset = Asset.fromURI(uri); await asset.downloadAsync();
  const gl = await GLView.createContextAsync();
  try {
    const shader = (type: number, source: string) => {
      const value = gl.createShader(type)!; gl.shaderSource(value, source); gl.compileShader(value);
      if (!gl.getShaderParameter(value, gl.COMPILE_STATUS)) throw new Error("Artwork shader compilation failed");
      return value;
    };
    const program = gl.createProgram()!;
    gl.attachShader(program, shader(gl.VERTEX_SHADER, "attribute vec2 p; varying vec2 uv; void main(){ uv=(p+1.0)*0.5; gl_Position=vec4(p,0.,1.); }"));
    gl.attachShader(program, shader(gl.FRAGMENT_SHADER, "precision mediump float; varying vec2 uv; uniform sampler2D image; void main(){ gl_FragColor=texture2D(image,vec2(uv.x,0.70+uv.y*0.29)); }"));
    gl.linkProgram(program); if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error("Artwork shader link failed");
    gl.useProgram(program);
    const input = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, input);
    gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MIN_FILTER,gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_MAG_FILTER,gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_WRAP_S,gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D,gl.TEXTURE_WRAP_T,gl.CLAMP_TO_EDGE);
    gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,gl.RGBA,gl.UNSIGNED_BYTE,{localUri:asset.localUri || asset.uri} as any);
    const target = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D,target);
    gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,16,16,0,gl.RGBA,gl.UNSIGNED_BYTE,null);
    const fbo=gl.createFramebuffer(); gl.bindFramebuffer(gl.FRAMEBUFFER,fbo);
    gl.framebufferTexture2D(gl.FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,target,0);
    if(gl.checkFramebufferStatus(gl.FRAMEBUFFER)!==gl.FRAMEBUFFER_COMPLETE) throw new Error("Artwork framebuffer unavailable");
    gl.bindBuffer(gl.ARRAY_BUFFER,gl.createBuffer()); gl.bufferData(gl.ARRAY_BUFFER,new Float32Array([-1,-1,1,-1,-1,1,1,1]),gl.STATIC_DRAW);
    const loc=gl.getAttribLocation(program,"p"); gl.enableVertexAttribArray(loc); gl.vertexAttribPointer(loc,2,gl.FLOAT,false,0,0);
    gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D,input); gl.uniform1i(gl.getUniformLocation(program,"image"),0);
    gl.viewport(0,0,16,16); gl.drawArrays(gl.TRIANGLE_STRIP,0,4);
    const pixels=new Uint8Array(16*16*4); gl.readPixels(0,0,16,16,gl.RGBA,gl.UNSIGNED_BYTE,pixels);
    const sums=[0,0,0]; let weight=0;
    for(let i=0;i<pixels.length;i+=4){const a=pixels[i+3]!/255;weight+=a;for(let c=0;c<3;c++)sums[c]=sums[c]!+pixels[i+c]!*a;}
    if(weight<1) throw new Error("Artwork sample empty");
    // Darken the sampled hue for readable controls, preserve color ratios.
    const rgb=sums.map(v=>Math.round(v/weight*.52));
    const color=`rgb(${rgb.join(",")})`; cache.set(uri,color);
    if(cache.size>24)cache.delete(cache.keys().next().value!);
    return color;
  } finally { await GLView.destroyContextAsync(gl); }
}

