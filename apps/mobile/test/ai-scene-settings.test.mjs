import assert from "node:assert/strict";
import { test } from "node:test";
import { createRequire } from "node:module";
import { build } from "esbuild";
const require = createRequire(import.meta.url);
const contextBundle = await build({ entryPoints: ["apps/mobile/src/ai-scene-context.tsx"], bundle: true, platform: "node", format: "cjs", external: ["*"], write: false });
const sessionBundle = await build({ entryPoints: ["apps/mobile/src/ai-scene-session.ts"], bundle: true, platform: "node", format: "cjs", write: false });
const modelBundle = await build({ entryPoints: ["apps/mobile/src/ai-scene.ts"], bundle: true, platform: "node", format: "cjs", write: false });
const uiBundle = await build({ entryPoints: ["apps/mobile/src/AiSceneSettings.tsx"], bundle: true, platform: "node", format: "cjs", external: ["*"], write: false });
const moduleFor = (bundle, mock = {}) => { const module = { exports: {} }; new Function("require", "module", "exports", bundle.outputFiles[0].text)(name => mock[name] ?? require(name), module, module.exports); return module.exports; };
const model = moduleFor(modelBundle), session = moduleFor(sessionBundle);
const tick = () => new Promise(resolve => setImmediate(resolve));
const node = (type, props, ...children) => ({ type, props: { ...props, children } });
const nodes = tree => !tree || typeof tree !== "object" ? [] : Array.isArray(tree) ? tree.flatMap(nodes) : [tree, ...nodes(tree.props?.children)];
function hooks() {
  let slots = [], cursor = 0, effects = [];
  return { begin() { cursor = 0; }, flush() { const pending = effects; effects = []; pending.forEach(fn => fn()); }, react: {
    createElement: node, createContext: value => ({ Provider: "Provider", value }),
    useState(initial) { const index = cursor++; if (!(index in slots)) slots[index] = typeof initial === "function" ? initial() : initial; return [slots[index], next => { slots[index] = typeof next === "function" ? next(slots[index]) : next; }]; },
    useRef(initial) { const index = cursor++; if (!(index in slots)) slots[index] = { current: initial }; return slots[index]; },
    useSyncExternalStore(_, snapshot) { cursor++; return snapshot(); },
    useEffect(fn, deps) { const index = cursor++, before = slots[index]; if (!before || deps.some((d, i) => d !== before.deps[i])) { slots[index] = { deps }; effects.push(() => { before?.cleanup?.(); slots[index].cleanup = fn(); }); } },
  } };
}
function storageHarness(options = {}) {
  const h = hooks(), writes = [], keys = [], listeners = new Set();
  let key = options.rawCredential ?? (options.key ? JSON.stringify({ baseUrl: options.settings?.baseUrl || "", apiKey: options.key }) : ""), config = JSON.stringify(options.settings ?? {});
  const secure = { WHEN_UNLOCKED_THIS_DEVICE_ONLY: "device-only", async getItemAsync() { if (options.readKey) return options.readKey; return key; }, async setItemAsync(id, value, opts) { keys.push({ id, value: JSON.parse(value).apiKey, raw: value, opts }); if (options.failKey) throw new Error("private sensitive token in native error"); key = value; }, async deleteItemAsync(id) { keys.push({ id, deleted: true }); key = ""; } };
  const fs = { documentDirectory: "file:///documents/", async readAsStringAsync() { return config; }, async writeAsStringAsync(path, value) { if (options.failFile) throw new Error("disk full"); config = value; writes.push({ path, value }); } };
  const appState = { currentState: "active", addEventListener(_, listener) { listeners.add(listener); return { remove() { listeners.delete(listener); } }; } };
  const api = moduleFor(contextBundle, { react: h.react, "react-native": { AppState: appState }, "expo-file-system/legacy": fs, "expo-secure-store": secure, "./ai-scene": model, "./ai-scene-session": options.request ? { AiSceneSession: class extends session.AiSceneSession { constructor() { super(options.request); } } } : session, "./AiSceneSettings": { AiSceneSettingsModal: "SceneModal" } });
  return { writes, keys, render() { h.begin(); const tree = api.AiSceneProvider({ song: { id: "song", title: "Song" }, children: "player" }); h.flush(); return tree.props.value; }, background() { appState.currentState = "background"; listeners.forEach(fn => fn("background")); }, get key() { return key ? JSON.parse(key).apiKey : ""; } };
}
const settings = { ...model.AI_SCENE_DEFAULTS, baseUrl: "https://api.example.test/v1", model: "my-model" };
test("key is written only to secure storage and settings never contain credentials or generated/audio data", async () => {
  const h = storageHarness(); assert.equal(h.render().ready, false); await tick(); let v = h.render(); assert.equal(v.ready, true);
  assert.equal(await v.save(settings, "private-test-key"), true); v = h.render();
  assert.equal(v.hasApiKey, true); assert.equal(h.key, "private-test-key"); assert.equal(h.keys[0].opts.keychainAccessible, "device-only");
  assert.equal(h.writes.length, 1); assert.deepEqual(JSON.parse(h.writes[0].value), settings); assert.equal(h.writes[0].value.includes("private-test-key"), false);
  assert.equal("apiKey" in v, false); assert.equal("scene" in JSON.parse(h.writes[0].value), false);
  assert.equal(await v.save({ ...settings, prompt: "new preference" }), true); assert.equal(h.keys.length, 1);
});
test("late secure-store load cannot be overwritten by editing before ready", async () => {
  let resolve; const readKey = new Promise(r => { resolve = r; }); const h = storageHarness({ readKey, settings });
  const before = h.render(); assert.equal(await before.save(settings, "early-key"), false); assert.equal(h.keys.length, 0);
  resolve(JSON.stringify({ baseUrl: settings.baseUrl, apiKey: "saved-key" })); await tick(); const loaded = h.render(); assert.equal(loaded.hasApiKey, true); assert.equal(loaded.settings.baseUrl, settings.baseUrl); assert.equal(h.writes.length, 0);
});
test("secure-store failures never fall back to a plaintext key or expose native error details", async () => {
  const h = storageHarness({ failKey: true }); h.render(); await tick(); const v = h.render(); assert.equal(await v.save(settings, "private-key"), false);
  const after = h.render(); assert.equal(after.hasApiKey, false); assert.equal(h.writes.length, 0); assert.match(after.preferenceError, /安全保存/); assert.equal(after.preferenceError.includes("sensitive"), false);
});
test("config-write failure restores the previous secure key and leaves the prior settings in effect", async () => {
  const h = storageHarness({ key: "old-key", settings, failFile: true }); h.render(); await tick(); const v = h.render();
  assert.equal(await v.save({ ...settings, model: "changed" }, "new-key"), false); assert.equal(h.key, "old-key");
  assert.equal(h.render().settings.model, "my-model"); assert.deepEqual(h.keys.map(x => x.value), ["new-key", "old-key"]);
});
test("clearing credentials disables generation and deletes the secure item", async () => {
  const h = storageHarness({ key: "old-key", settings }); h.render(); await tick(); const v = h.render();
  assert.equal(await v.save({ ...settings, enabled: false }, undefined, true), true); assert.equal(h.key, ""); assert.equal(h.render().hasApiKey, false); assert.equal(h.keys.at(-1).deleted, true);
});
test("both mobile systems use the same modal, conceal stored key and save drafts before generation", async () => {
  const h = hooks(), events = [];
  const native = Object.fromEntries(["Modal", "SafeAreaView", "KeyboardAvoidingView", "View", "Text", "TextInput", "Pressable", "ScrollView", "Switch", "ActivityIndicator"].map(name => [name, name]));
  Object.assign(native, { Platform: { OS: "ios" }, useColorScheme: () => "dark", StyleSheet: { create: x => x } });
  const api = moduleFor(uiBundle, { react: h.react, "react-native": native, "./app-theme": { useAppTheme: () => ({ accent: "#7799CC" }) }, "react-native-safe-area-context": { SafeAreaProvider: "SafeAreaProvider", SafeAreaView: "SafeAreaView", initialWindowMetrics: {} } });
  const value = { settings: { ...settings, enabled: true }, settingsOpen: true, ready: true, saving: false, hasApiKey: true, pending: false, scene: null, error: "", preferenceError: "", async save(draft, key) { events.push(["save", draft, key]); return true; }, regenerate() { events.push(["generate"]); }, closeSettings() { events.push(["close"]); }, cancel() {} };
  const render = () => { h.begin(); const tree = api.AiSceneSettingsModal({ value }); h.flush(); return tree; };
  render(); let tree = render(), fields = nodes(tree).filter(n => n.type === "TextInput");
  const input = fields.find(n => n.props.accessibilityLabel === "API Key"); assert.equal(input.props.value, ""); assert.equal(input.props.secureTextEntry, true); assert.match(input.props.placeholder, /已安全保存/);
  input.props.onChangeText("new-secret"); tree = render(); nodes(tree).find(n => n.props.testID === "ai-scene-generate").props.onPress(); await tick();
  assert.deepEqual(events.map(e => e[0]), ["save", "generate"]); assert.equal(events[0][2], "new-secret");
  native.Platform.OS = "android"; tree = render(); assert.equal(nodes(tree).filter(n => n.props?.testID === "ai-scene-generate").length, 1);
});

test("saving changed credentials then generating manually survives the next React settings effect", async () => {
  let resolve, signal; const response = new Promise(r => { resolve = r; });
  const h = storageHarness({ request: async (_settings, _key, _song, requestSignal) => { signal = requestSignal; return response; } });
  h.render(); await tick(); const v = h.render();
  assert.equal(await v.save({ ...settings, enabled: true, autoGenerate: false }, "new-key"), true);
  const generation = v.regenerate(); h.render();
  assert.equal(signal.aborted, false, "preference effect must not abort a current manual request");
  resolve({ name: "manual scene" }); await generation; assert.equal(h.render().scene.name, "manual scene");
});

test("a key left behind by a failed provider change cannot be loaded for a different service", async () => {
  const h = storageHarness({ settings, rawCredential: JSON.stringify({ baseUrl: "https://other.example.test/v1", apiKey: "other-service-key" }) });
  h.render(); await tick(); const v = h.render(); assert.equal(v.hasApiKey, false); assert.match(v.preferenceError, /地址已变化/);
});
