import { AI_SCENE_DEFAULTS, requestAiScene, type AiSceneDescription, type AiSceneSettings, type AiSceneSong } from "./ai-scene";
export type AiSceneState = { trackId: string; scene: AiSceneDescription | null; pending: boolean; error: string };
/** One request and one current composition. No audio data or disk scene cache. */
export class AiSceneSession {
  private settings = AI_SCENE_DEFAULTS;
  private song: AiSceneSong = { id: "" };
  private apiKey = "";
  private signature = "";
  private epoch = 0;
  private controller: AbortController | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private state: AiSceneState = { trackId: "", scene: null, pending: false, error: "" };
  private listeners = new Set<() => void>();
  constructor(private readonly request = requestAiScene) {}
  subscribe = (callback: () => void) => { this.listeners.add(callback); return () => { this.listeners.delete(callback); }; };
  snapshot = () => this.state;
  private publish(value: Partial<AiSceneState>) { this.state = { ...this.state, ...value }; for (const notify of this.listeners) notify(); }
  cancel = () => {
    this.epoch++; this.controller?.abort(); this.controller = null;
    if (this.timer) clearTimeout(this.timer); this.timer = null;
    if (this.state.pending) this.publish({ pending: false });
  };
  configure(settings: AiSceneSettings, apiKey: string, song: AiSceneSong) {
    const signature = JSON.stringify([settings, apiKey, song]);
    if (signature === this.signature) return;
    this.cancel(); this.signature = signature; this.settings = settings; this.apiKey = apiKey; this.song = { ...song };
    this.publish({ trackId: song.id, scene: null, pending: false, error: "" });
    this.resume();
  }
  resume = () => {
    if (this.settings.enabled && this.settings.autoGenerate && this.song.id && this.song.title && this.apiKey && this.settings.baseUrl && this.settings.model && !this.state.scene && !this.state.pending && !this.state.error && !this.timer) {
      this.timer = setTimeout(() => { this.timer = null; void this.generate(); }, 500);
    }
  };
  generate = async () => {
    if (!this.settings.enabled) return;
    this.cancel();
    if (!this.song.id) { this.publish({ error: "请先打开一首歌曲" }); return; }
    const epoch = this.epoch, controller = new AbortController(); this.controller = controller;
    this.publish({ pending: true, error: "" });
    try {
      const scene = await this.request(this.settings, this.apiKey, this.song, controller.signal);
      if (epoch === this.epoch && !controller.signal.aborted) this.publish({ scene, pending: false });
    } catch (error) {
      if (epoch === this.epoch && !controller.signal.aborted) this.publish({ pending: false, error: error instanceof Error ? error.message : "场景生成失败，请重试" });
    } finally { if (this.controller === controller) this.controller = null; }
  };
}
