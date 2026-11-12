import assert from "node:assert/strict";
import { test } from "node:test";
import { build } from "esbuild";
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
const bundle = await build({ entryPoints: ["apps/mobile/src/ai-scene.ts", "apps/mobile/src/ai-scene-session.ts", "apps/mobile/src/ai-scene-layout.ts"], outdir: "unused", bundle: true, platform: "node", format: "esm", write: false });
const load = async name => import("data:text/javascript;base64," + Buffer.from(bundle.outputFiles.find(f => f.path.endsWith(name + ".js")).text).toString("base64"));
const ai = await load("ai-scene"), { AiSceneSession } = await load("ai-scene-session"), { aiSceneParticlePositions } = await load("ai-scene-layout");
const composition = { version: 2, name: "潮汐折光", themes: {
  light: { backgroundColor: "#e8e0db", primaryColor: "#302c38", secondaryColor: "#534b59", accentColor: "#755674", colors: ["#b9c8cf", "#d9b2c6", "#acc7c1"] },
  dark: { backgroundColor: "#252535", primaryColor: "#f3eef8", secondaryColor: "#cdc4dc", accentColor: "#c995c2", colors: ["#667dae", "#af729e", "#67a1a4"] }
}, atmosphere: { diffusion: .8, flow: .05 }, particles: { count: 40 } };
const song = { id: "file:///private/sensitive/path/music.m4a", title: "歌名", artist: "歌手", album: "专辑", year: "2026", coverUri: "file:///private/cover.jpg", lyrics: "PRIVATE LYRICS", audio: "PRIVATE AUDIO" };
const settings = { ...ai.AI_SCENE_DEFAULTS, enabled: true, autoGenerate: false, baseUrl: "https://example.invalid/v1", model: "user-model", prompt: "空间自由些" };
const tick = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
const respond = value => new Response(JSON.stringify({ choices: [{ message: { content: JSON.stringify(value) } }] }), { headers: { "content-type": "application/json" } });
test("AI is opt-in and custom endpoints preserve prefixes and already-complete routes", () => {
  assert.equal(ai.AI_SCENE_DEFAULTS.enabled, false);
  for (const input of ["https://host.test", "https://host.test/", "https://host.test/v1/", "https://host.test/v1/chat/completions"]) assert.equal(ai.aiSceneEndpoint(input), "https://host.test/v1/chat/completions");
  assert.equal(ai.aiSceneEndpoint("http://192.168.1.2:8080/api/v2/"), "http://192.168.1.2:8080/api/v2/chat/completions");
  for (const input of ["host.test/v1", "file:///tmp/key", "javascript:alert(1)", "https://user:secret@host.test/v1", "https://host.test/v1?key=secret", "https://host.test/v1#fragment"]) assert.throws(() => ai.aiSceneEndpoint(input));
  const clean = ai.normalizeAiSceneSettings({ ...settings, enabled: "true", apiKey: "PRIVATE" }); assert.equal(clean.enabled, false); assert.equal("apiKey" in clean, false);
});
test("AI generates dual visual themes instead of a mesh arrangement or executable code", () => {
  const scene = ai.parseAiScene("```json\n" + JSON.stringify({ ...composition, script: "throw Error('run')", elements: [{ shape: "box" }], assetUrl: "https://bad.test" }) + "\n```", 123);
  assert.equal(scene.name, "潮汐折光"); assert.equal(scene.version, 2);
  for (const field of ["elements", "script", "assetUrl", "environment", "field"]) assert.equal(field in scene, false);
  assert.notDeepEqual(scene.themes.light, scene.themes.dark);
  for (const input of ["not json", "{}", "[]", '{"version":2}', "x".repeat(48001)]) assert.throws(() => ai.parseAiScene(input, 1));
});
test("legacy responses migrate to color themes and discard every old decorative mesh", () => {
  const old = { version: 1, name: "旧主题", background: "#000000", palette: ["#73486b", "#46939e", "#cf9865"], elements: Array.from({ length: 24 }, () => ({ shape: "sphere", scale: [4,4,4], position: [0,0,0] })) };
  const scene = ai.parseAiScene(JSON.stringify(old), 7);
  assert.equal(scene.version, 2); assert.equal("elements" in scene, false);
  assert(scene.themes.dark.colors.some(c => c !== "#000000"));
  assert.doesNotThrow(() => ai.selectAiSceneTheme({ ...old, seed: 7 }, "light"));
});
test("invalid model numbers stay bounded and bright/dark themes preserve readable contrast", () => {
  const scene = ai.parseAiScene(JSON.stringify({ ...composition, atmosphere: { scale: 1e8, flow: 8, diffusion: -10, glow: 20, grain: 9 }, particles: { count: 1e9, size: 1e9, speed: 1e9, spread: [-1, 1e9, "wrong"] }, themes: {
    light: { backgroundColor: "#000000", primaryColor: "#ffffff", secondaryColor: "#ffffff", colors: ["#000000", "#ffffff", "bad"] },
    dark: { backgroundColor: "#ffffff", primaryColor: "#000000", secondaryColor: "#000000", colors: ["#000000", "#ffffff", "bad"] }
  } }), 1);
  assert.equal(scene.particles.count, 96); assert.equal(scene.atmosphere.flow, .16); assert.equal(scene.atmosphere.grain, .035); assert.equal(scene.atmosphere.diffusion, .35);
  assert.deepEqual(scene.particles.spread, [6, 20, 12]);
  for (const mode of ["light", "dark"]) { const theme = scene.themes[mode]; assert(ai.sceneColorContrast(theme.primaryColor, theme.backgroundColor) >= 4.5); assert(ai.sceneColorContrast(theme.secondaryColor, theme.backgroundColor) >= 4.5); }
  assert(ai.sceneColorLuminance(scene.themes.light.backgroundColor) >= .54);
  assert(ai.sceneColorLuminance(scene.themes.dark.backgroundColor) >= .017 && ai.sceneColorLuminance(scene.themes.dark.backgroundColor) <= .091);
});
test("particle geometry stays outside source positions and is stable for the current composition", () => {
  const scene = ai.parseAiScene(JSON.stringify(composition), ai.sceneSeed("song A"));
  const a = aiSceneParticlePositions(scene), b = aiSceneParticlePositions(scene);
  assert.deepEqual(a, b); assert.equal(a.length, 120);
  assert.notDeepEqual(a, aiSceneParticlePositions({ ...scene, seed: ai.sceneSeed("song B") }));
  for (let i = 0; i < a.length; i += 3) assert(Math.hypot(a[i], a[i + 1], a[i + 2]) >= 3.99999);
});
test("actual HTTP-compatible request sends only selected metadata, with the user's model and key", async () => {
  let captured;
  const server = createServer(async (req, res) => { const chunks = []; for await (const chunk of req) chunks.push(chunk); captured = { url: req.url, authorization: req.headers.authorization, body: JSON.parse(Buffer.concat(chunks)) }; res.setHeader("content-type", "application/json"); res.end(JSON.stringify({ choices: [{ message: { content: JSON.stringify(composition) } }] })); });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  try {
    const port = server.address().port;
    const result = await ai.requestAiScene({ ...settings, baseUrl: `http://127.0.0.1:${port}/custom/v1` }, "session-key", song, new AbortController().signal);
    assert.equal(result.name, composition.name); assert.equal(captured.url, "/custom/v1/chat/completions"); assert.equal(captured.authorization, "Bearer session-key"); assert.equal(captured.body.model, "user-model");
    const sent = JSON.stringify(captured.body); for (const privateText of [song.id, song.coverUri, song.lyrics, song.audio, "session-key"]) assert.equal(sent.includes(privateText), false);
    assert.deepEqual(JSON.parse(captured.body.messages[1].content).song, { title: song.title, artist: song.artist, album: song.album, year: song.year });
  } finally { await new Promise(resolve => server.close(resolve)); }
});
test("authentication, provider errors, malformed JSON and refusals are safe and actionable", async () => {
  for (const status of [401, 403, 429, 500]) await assert.rejects(ai.requestAiScene(settings, "secret-key", song, new AbortController().signal, async () => new Response("private provider details secret-key", { status })), error => !error.message.includes("secret-key") && /认证|限流|HTTP/.test(error.message));
  await assert.rejects(ai.requestAiScene(settings, "secret-key", song, new AbortController().signal, async () => { throw new Error("AI private secret-key"); }), /无法连接/);
  for (const raw of ["html", '{"choices":[]}', '{"choices":[{"message":{"refusal":"no"}}]}']) await assert.rejects(ai.requestAiScene(settings, "key", song, new AbortController().signal, async () => new Response(raw)));
  await assert.rejects(ai.requestAiScene(settings, "key", song, new AbortController().signal, async () => new Response("x", { headers: { "content-length": "100000" } })), /过大/);
});
test("requests accept textual content parts and propagate cancellation to the network", async () => {
  const controller = new AbortController(); let networkSignal;
  const p = ai.requestAiScene(settings, "key", song, controller.signal, async (_, options) => { networkSignal = options.signal; return new Promise((_, reject) => options.signal.addEventListener("abort", () => reject(new Error("native aborted")))); });
  controller.abort(); await assert.rejects(p, /取消/); assert.equal(networkSignal.aborted, true);
  const scene = await ai.requestAiScene(settings, "key", song, new AbortController().signal, async () => new Response(JSON.stringify({ choices: [{ message: { content: [{ type: "text", text: JSON.stringify(composition) }] } }] })));
  assert.equal(scene.name, composition.name);
});
test("network timeout clears the in-flight request without blocking playback", async () => {
  const previous = global.setTimeout; global.setTimeout = (fn, delay, ...args) => previous(fn, delay === 30000 ? 2 : delay, ...args);
  try { await assert.rejects(ai.requestAiScene(settings, "key", song, new AbortController().signal, async (_, options) => new Promise((_, reject) => options.signal.addEventListener("abort", () => reject(new Error("timeout"))))), /超时/); }
  finally { global.setTimeout = previous; }
});
test("switching songs rejects stale responses even when a transport ignores abort", async () => {
  const requests = [], session = new AiSceneSession((...args) => { const d = deferred(); requests.push({ ...d, args }); return d.promise; });
  session.configure(settings, "key", song); const first = session.generate();
  session.configure(settings, "key", { ...song, id: "track-b", title: "B" }); const second = session.generate();
  assert.equal(requests[0].args[3].aborted, true); requests[1].resolve({ name: "B" }); await second; requests[0].resolve({ name: "A" }); await first;
  assert.equal(session.snapshot().scene.name, "B"); assert.equal(session.snapshot().trackId, "track-b"); assert.equal(session.snapshot().pending, false);
});
test("cancel, disabling AI and regeneration failures cannot replace or clear the wrong scene", async () => {
  let failure = false; const session = new AiSceneSession(async () => { if (failure) throw new Error("AI 服务限流"); return { name: "current" }; });
  session.configure(settings, "key", song); await session.generate(); failure = true; await session.generate();
  assert.equal(session.snapshot().scene.name, "current"); assert.match(session.snapshot().error, /限流/);
  session.configure({ ...settings, enabled: false }, "key", song); assert.equal(session.snapshot().scene, null); await session.generate(); assert.equal(session.snapshot().pending, false);
});
test("auto generation debounces metadata, avoids playback-tick duplicates and never retries failures in a loop", async () => {
  let calls = 0; const session = new AiSceneSession(async () => { calls++; throw new Error("AI service unavailable"); });
  const automatic = { ...settings, autoGenerate: true };
  session.configure(automatic, "key", song); session.configure(automatic, "key", { ...song });
  session.configure(automatic, "key", { ...song, title: "settled" });
  await new Promise(resolve => setTimeout(resolve, 550)); assert.equal(calls, 1);
  session.configure(automatic, "key", { ...song, title: "settled" }); session.resume(); await new Promise(resolve => setTimeout(resolve, 550)); assert.equal(calls, 1); session.cancel();
});
test("background cancellation resumes the same song without retaining old-track results", async () => {
  let calls = 0; const session = new AiSceneSession(async () => { calls++; return { name: "resumed" }; });
  session.configure({ ...settings, autoGenerate: true }, "key", song); session.cancel(); await tick(); assert.equal(calls, 0);
  session.resume(); await new Promise(resolve => setTimeout(resolve, 550)); assert.equal(calls, 1); assert.equal(session.snapshot().scene.name, "resumed"); session.cancel();
});
test("generated decoration keeps real object rendering, source positions, silent hiding and audio ownership intact", () => {
  const source = readFileSync("apps/mobile/src/MobileObjectScene.tsx", "utf8"), renderer = readFileSync("apps/mobile/src/AiSceneEnvironment.tsx", "utf8");
  assert.match(source, /<AiSceneEnvironment/); assert.match(source, /<BatchedSpaceObjects objects=\{objects\}/); assert.match(source, /MobileObjectActivity/);
  assert.match(source, /frameloop=\{renderActive \? "always" : "never"\}/);
  assert.doesNotMatch(renderer, /eval\(|new Function|fetch\(|setState\(|setInterval\(/);
  assert.doesNotMatch(renderer, /setVolume|setHeadYaw|seekTo|pause\(|stop\(/);
  const settingsNative = readFileSync("apps/mobile/src/IOSNativeSettings.tsx", "utf8"), settingsAndroid = readFileSync("apps/mobile/src/AndroidSettings.tsx", "utf8");
  assert.match(settingsNative, /onPress=\{aiScene.openSettings\}/); assert.match(settingsAndroid, /onPress=\{aiScene.openSettings\}/);
});

test("newer compatible models can request the alternate token-limit field without an unbounded retry", async () => {
  const bodies = []; const result = await ai.requestAiScene(settings, "key", song, new AbortController().signal, async (_, options) => {
    bodies.push(JSON.parse(options.body));
    if (bodies.length === 1) return new Response(JSON.stringify({ error: { message: "Unsupported max_tokens; use max_completion_tokens" } }), { status: 400 });
    return respond(composition);
  });
  assert.equal(result.name, composition.name); assert.equal(bodies.length, 2); assert.equal(bodies[1].max_completion_tokens, bodies[0].max_tokens); assert.equal("max_tokens" in bodies[1], false);
});
