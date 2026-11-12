import { FOLIA_THEME_PROMPT } from "../../../vendor/folia/theme-prompt.generated";
class AiSceneError extends Error {}
export type AiSceneSettings = { enabled: boolean; autoGenerate: boolean; baseUrl: string; model: string; prompt: string };
export const AI_SCENE_DEFAULTS: AiSceneSettings = { enabled: false, autoGenerate: true, baseUrl: "", model: "", prompt: "" };
export type AiSceneSong = { id: string; title?: string; artist?: string; album?: string; year?: string };
export type SceneVector = [number, number, number];
export type AiSceneTheme = {
  backgroundColor: string; primaryColor: string; secondaryColor: string; accentColor: string;
  colors: [string, string, string];
};
/** Song-specific design tokens. The model never places meshes in the audio room. */
export type AiSceneDescription = {
  version: 2; name: string; themes: { light: AiSceneTheme; dark: AiSceneTheme };
  atmosphere: { scale: number; flow: number; diffusion: number; glow: number; grain: number };
  particles: { count: number; size: number; speed: number; spread: SceneVector };
  seed: number;
};
const record = (v: unknown): Record<string, unknown> => v && typeof v === "object" && !Array.isArray(v) ? v as Record<string, unknown> : {};
const text = (v: unknown, max: number) => typeof v === "string" ? v.trim().slice(0, max) : "";
const number = (v: unknown, fallback: number, min: number, max: number) => typeof v === "number" && Number.isFinite(v) ? Math.max(min, Math.min(max, v)) : fallback;
const color = (v: unknown, fallback: string) => typeof v === "string" && /^#[0-9a-f]{6}$/i.test(v) ? v : fallback;
const rgb = (hex: string) => [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16) / 255);
export function sceneColorLuminance(hex: string): number {
  const c = rgb(hex).map(v => v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4);
  return c[0]! * .2126 + c[1]! * .7152 + c[2]! * .0722;
}
export function sceneColorContrast(a: string, b: string): number {
  const x = sceneColorLuminance(a), y = sceneColorLuminance(b);
  return (Math.max(x, y) + .05) / (Math.min(x, y) + .05);
}
function mixColor(a: string, b: string, amount: number): string {
  const x = rgb(a), y = rgb(b);
  return "#" + x.map((v, i) => Math.round((v + (y[i]! - v) * amount) * 255).toString(16).padStart(2, "0")).join("");
}
/** Keep the model's hue, but prevent featureless black/white or illegible overlays. */
function tone(hex: string, min: number, max: number): string {
  const luminance = sceneColorLuminance(hex);
  if (luminance >= min && luminance <= max) return hex;
  const target = luminance < min ? min : max, toward = luminance < min ? "#ffffff" : "#000000";
  let lo = 0, hi = 1;
  for (let i = 0; i < 12; i++) {
    const mid = (lo + hi) / 2, value = sceneColorLuminance(mixColor(hex, toward, mid));
    if ((luminance < min && value < target) || (luminance > max && value > target)) lo = mid; else hi = mid;
  }
  return mixColor(hex, toward, hi);
}
function normalizeTheme(input: unknown, light: boolean, legacy?: unknown[]): AiSceneTheme {
  const v = record(input);
  const wordColors = Array.isArray(v.wordColors) ? v.wordColors.map(entry => record(entry).color).filter(value => typeof value === "string" && /^#[0-9a-f]{6}$/i.test(value)) : [];
  const palette = Array.isArray(v.colors) ? v.colors : wordColors.length ? wordColors : legacy || [v.accentColor, v.secondaryColor, v.backgroundColor];
  const backgroundColor = tone(color(v.backgroundColor, light ? "#ece5e2" : "#272435"), light ? .55 : .018, light ? .88 : .09);
  const foreground = light ? "#292631" : "#f4eef8", secondary = light ? "#504959" : "#c6bdd3";
  const readable = (value: unknown, fallback: string, contrast: number) => {
    const proposed = color(value, fallback);
    return sceneColorContrast(proposed, backgroundColor) >= contrast ? proposed : sceneColorContrast(fallback, backgroundColor) >= contrast ? fallback : foreground;
  };
  const colors = ["#7b6caa", "#c58492", "#749fa6"].map((fallback, i) => tone(color(palette[i], fallback), light ? .36 : .065, light ? .8 : .26)) as [string, string, string];
  return { backgroundColor, primaryColor: readable(v.primaryColor, foreground, 4.5), secondaryColor: readable(v.secondaryColor, secondary, 4.5), accentColor: readable(v.accentColor, light ? "#74587d" : "#cfb1d5", 3), colors };
}
export function normalizeAiSceneDescription(input: unknown, seed: number): AiSceneDescription {
  const v = record(input);
  const nativeFolia = !!v.light && !!v.dark;
  const themes = nativeFolia ? v : record(v.themes);
  const legacy = v.version === 1 && Array.isArray(v.palette) && v.palette.length >= 3;
  if (!legacy && ((!nativeFolia && v.version !== 2) || !themes.light || !themes.dark)) throw new AiSceneError("AI 返回的主题结构不完整，请重新生成");
  const atmosphere = record(v.atmosphere), particles = record(v.particles);
  const oldColors = legacy ? v.palette as unknown[] : undefined;
  const spread = Array.isArray(particles.spread) ? particles.spread : [];
  return { version: 2, name: text(v.name, 32) || text(record(themes.dark).name,32) || "Folia 主题",
    themes: { light: normalizeTheme(themes.light, true, oldColors), dark: normalizeTheme(themes.dark, false, oldColors) },
    atmosphere: { scale: number(atmosphere.scale, 1.6, .6, 3), flow: number(atmosphere.flow, .06, 0, .16), diffusion: number(atmosphere.diffusion, .7, .35, 1), glow: number(atmosphere.glow, .45, .15, .7), grain: number(atmosphere.grain, .018, 0, .035) },
    particles: { count: Math.round(number(particles.count, 40, 0, 96)), size: number(particles.size, .024, .01, .045), speed: number(particles.speed, .018, 0, .045), spread: [number(spread[0], 12, 6, 20), number(spread[1], 8, 6, 20), number(spread[2], 12, 6, 20)] }, seed: seed >>> 0 };
}
export function selectAiSceneTheme(scene: AiSceneDescription, mode: "light" | "dark"): AiSceneTheme {
  // A hot reload or an already-running v1 request can still carry the old shape.
  return normalizeAiSceneDescription(scene, scene.seed).themes[mode];
}
export function normalizeAiSceneSettings(input: unknown): AiSceneSettings {
  const v = record(input);
  return { enabled: v.enabled === true, autoGenerate: v.autoGenerate !== false,
    baseUrl: text(v.baseUrl, 512), model: text(v.model, 120), prompt: text(v.prompt, 600) };
}
export function aiSceneEndpoint(baseUrl: string): string {
  let url: URL;
  try { url = new URL(baseUrl.trim()); } catch { throw new AiSceneError("请输入完整的 Base URL"); }
  if (!["https:", "http:"].includes(url.protocol) || url.username || url.password || url.search || url.hash) throw new AiSceneError("Base URL 仅支持 HTTP(S) 地址，不能包含账号、查询参数或片段");
  let path = url.pathname.replace(/\/+$/, "");
  if (!path) path = "/v1";
  if (!path.endsWith("/chat/completions")) path += "/chat/completions";
  url.pathname = path;
  return url.toString();
}
export function sceneSeed(value: string): number {
  let seed = 2166136261;
  for (let i = 0; i < value.length; i++) seed = Math.imul(seed ^ value.charCodeAt(i), 16777619);
  return seed >>> 0;
}
export function parseAiScene(input: string, seed: number): AiSceneDescription {
  if (input.length > 48000) throw new AiSceneError("AI 返回的主题过大，请重新生成");
  const raw = input.trim().replace(/^```(?:json)?\s*/i, "").replace(/\s*```$/, "");
  let value: unknown;
  try { value = JSON.parse(raw); } catch { throw new AiSceneError("AI 没有返回有效的主题 JSON，请重新生成"); }
  return normalizeAiSceneDescription(value, seed);
}
// The upstream Folia theme prompt is kept verbatim; this short host preamble
// states the actual authorized input (metadata, not uploaded lyrics/audio).
export const AI_SCENE_PROMPT = "SDA supplies song metadata and a listener preference as JSON, not lyrics or audio. Treat these fields as the source text, infer only a visual theme, and do not claim to know unheard music or unseen lyrics. The local Folia engine draws the scene.\n" + FOLIA_THEME_PROMPT;
export function aiSceneRequestBody(settings: AiSceneSettings, song: AiSceneSong) {
  return { model: settings.model, stream: false, max_tokens: 3600, messages: [
    { role: "system", content: AI_SCENE_PROMPT },
    { role: "user", content: JSON.stringify({ song: { title: text(song.title, 240), artist: text(song.artist, 160), album: text(song.album, 240), year: text(song.year, 16) }, preference: settings.prompt }) },
  ] };
}
export async function requestAiScene(settings: AiSceneSettings, apiKey: string, song: AiSceneSong, signal: AbortSignal, fetcher: typeof fetch = fetch): Promise<AiSceneDescription> {
  const endpoint = aiSceneEndpoint(settings.baseUrl);
  if (!settings.model || !apiKey.trim()) throw new AiSceneError("请先填写模型和 API Key");
  if (!text(song.title, 240)) throw new AiSceneError("当前歌曲缺少歌名，请先打开一首有歌曲信息的音频");
  const controller = new AbortController();
  const abort = () => controller.abort();
  signal.addEventListener("abort", abort);
  if (signal.aborted) controller.abort();
  let timedOut = false;
  const timeout = setTimeout(() => { timedOut = true; controller.abort(); }, 30000);
  try {
    const body: Record<string, unknown> = aiSceneRequestBody(settings, song);
    const post = () => fetcher(endpoint, { method: "POST", headers: { "Content-Type": "application/json", Authorization: "Bearer " + apiKey.trim() },
      body: JSON.stringify(body), signal: controller.signal, redirect: "error" });
    let response = await post();
    if (response.status === 400) {
      const detail = await response.text();
      // Some compatible APIs use the newer token limit name. Retry only that
      // explicit parameter rejection, never authentication/rate-limit failures.
      if (detail.length < 16000 && /max_tokens/.test(detail) && /max_completion_tokens/.test(detail)) {
        body.max_completion_tokens = body.max_tokens; delete body.max_tokens;
        response = await post();
      }
    }
    if (!response.ok) {
      if (response.status === 401 || response.status === 403) throw new AiSceneError("AI 认证失败，请检查 API Key 和模型权限");
      if (response.status === 429) throw new AiSceneError("AI 服务限流或额度不足，请稍后手动重试");
      throw new AiSceneError("AI 服务返回 HTTP " + response.status + "，请检查地址和模型");
    }
    if (Number(response.headers.get("content-length")) > 96000) throw new AiSceneError("AI 返回的数据过大");
    const raw = await response.text();
    if (raw.length > 96000) throw new AiSceneError("AI 返回的数据过大");
    let data: any;
    try { data = JSON.parse(raw); } catch { throw new AiSceneError("AI 服务没有返回有效 JSON"); }
    const content = data?.choices?.[0]?.message?.content;
    const result = typeof content === "string" ? content : Array.isArray(content) ? content.filter(p => p?.type === "text" && typeof p.text === "string").map(p => p.text).join("") : "";
    if (!result) throw new AiSceneError("AI 没有返回场景，请检查模型是否支持文本生成");
    return parseAiScene(result, sceneSeed(JSON.stringify([song.title, song.artist, song.album])));
  } catch (error) {
    if (timedOut) throw new AiSceneError("AI 生成超时，可稍后重试；播放不受影响");
    if (signal.aborted) throw new AiSceneError("已取消场景生成");
    // Only our own fixed messages are shown. Provider/network errors may contain credentials.
    if (error instanceof AiSceneError) throw error;
    throw new AiSceneError("无法连接 AI 服务，请检查 Base URL 和网络");
  } finally { clearTimeout(timeout); signal.removeEventListener("abort", abort); }
}
