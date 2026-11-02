const assert = require("node:assert/strict");
const fs = require("node:fs");
const ts = require("typescript");
const { test } = require("node:test");
const code = ts.transpileModule(fs.readFileSync(require("node:path").join(__dirname, "../src/use-progress-scrubber.ts"), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;

function player(overrides = {}) {
  const cells = [], effects = [], seeks = [], animations = [];
  let cursor = 0, options = { sourceKey: "song-a", durationMs: 120000, positionMs: 10000, disabled: false, seekTo: ms => seeks.push(ms), ...overrides };
  const changed = (a, b) => !a || a.some((value, i) => value !== b[i]);
  const hooks = {
    useRef(value) { const i = cursor++; return cells[i] ??= { current: value }; },
    useState(value) { const i = cursor++; cells[i] ??= { value }; return [cells[i].value, v => { cells[i].value = v; }]; },
    useMemo(fn, deps) { const i = cursor++; if (!cells[i] || changed(cells[i].deps, deps)) cells[i] = { value: fn(), deps }; return cells[i].value; },
    useEffect(fn, deps) { const i = cursor++; if (!cells[i] || changed(cells[i].deps, deps)) { const old = cells[i]; cells[i] = { deps }; effects.push(() => { old?.cleanup?.(); cells[i].cleanup = fn(); }); } },
  };
  const native = {
    Animated: { Value: class { stopAnimation() {} }, spring(_value, settings) { return { start() { animations.push(settings.toValue); } }; } },
    PanResponder: { create: handlers => ({ panHandlers: handlers }) },
  };
  const module = { exports: {} };
  new Function("require", "module", "exports", code)(name => name === "react" ? hooks : native, module, module.exports);
  let view;
  const render = (patch = {}) => { options = { ...options, ...patch }; cursor = 0; view = module.exports.useProgressScrubber(options); while (effects.length) effects.shift()(); cursor = 0; view = module.exports.useProgressScrubber(options); return view; };
  render();
  const event = x => ({ nativeEvent: { locationX: x } });
  const state = (dx = 0, numberActiveTouches = 1) => ({ dx, numberActiveTouches });
  return {
    seeks, animations, render,
    layout(width) { view.onLayout({ nativeEvent: { layout: { width } } }); return render(); },
    grant(x, touches = 1) { view.panHandlers.onPanResponderGrant(event(x), state(0, touches)); return render(); },
    move(dx, touches = 1) { view.panHandlers.onPanResponderMove(event(9999), state(dx, touches)); return render(); },
    release(dx = 0) { view.panHandlers.onPanResponderRelease(event(9999), state(dx, 0)); return render(); },
    cancel() { view.panHandlers.onPanResponderTerminate(); return render(); },
    unmount() { for (const cell of cells) cell?.cleanup?.(); },
  };
}

test("long hold previews without seeking, drag follows finger, release submits once", () => {
  const p = player(); p.layout(300);
  assert.equal(p.grant(75).positionMs, 30000);
  assert.equal(p.render({ positionMs: 14000 }).positionMs, 30000, "playback polling cannot overwrite held preview");
  assert.equal(p.move(75).positionMs, 60000);
  assert.deepEqual(p.seeks, []);
  assert.equal(p.animations.at(-1), 1.75);
  assert.equal(p.release(90).isScrubbing, false);
  assert.deepEqual(p.seeks, [66000]);
  assert.equal(p.animations.at(-1), 1);
  p.release(90); assert.deepEqual(p.seeks, [66000]);
});
test("tap still seeks immediately on release", () => {
  const p = player(); p.layout(300); p.grant(150); p.release(); assert.deepEqual(p.seeks, [60000]);
});
test("dragging outside clamps both ends and avoids seeking past EOF", () => {
  const p = player(); p.layout(300); p.grant(150); assert.equal(p.move(-500).positionMs, 0); p.release(-500);
  p.grant(150); assert.equal(p.move(500).positionMs, 119999); p.release(500); assert.deepEqual(p.seeks, [0, 119999]);
});
test("normal playback ticks and changing callback do not recreate the responder", () => {
  const p = player(); const first = p.layout(300).panHandlers; p.grant(75);
  const calls = []; const next = p.render({ positionMs: 22000, seekTo: ms => calls.push(ms) });
  assert.equal(next.panHandlers, first); p.move(75); p.release(75); assert.deepEqual(calls, [60000]); assert.deepEqual(p.seeks, []);
});
test("changing song while dragging cancels and cannot seek the new song", () => {
  const p = player(); p.layout(300); p.grant(75); assert.equal(p.render({ sourceKey: "song-b" }).isScrubbing, false); p.release(50); assert.deepEqual(p.seeks, []);
});
test("duration change, preparation and layout rotation cancel safely", () => {
  for (const action of [p => p.render({ durationMs: 60000 }), p => p.render({ disabled: true }), p => p.layout(600)]) {
    const p = player(); p.layout(300); p.grant(75); assert.equal(action(p).isScrubbing, false); p.release(50); assert.deepEqual(p.seeks, []);
  }
});
test("multitouch and OS gesture cancellation never seek", () => {
  for (const action of [p => p.move(10, 2), p => p.cancel()]) {
    const p = player(); p.layout(300); p.grant(75); action(p); p.release(50); assert.deepEqual(p.seeks, []);
  }
});
test("unknown duration, missing source, disabled state and invalid geometry cannot seek", () => {
  for (const options of [{ durationMs: 0 }, { durationMs: NaN }, { sourceKey: "" }, { disabled: true }, { seekTo: undefined }]) {
    const p = player(options); p.layout(300); p.grant(75); p.release(); assert.deepEqual(p.seeks, []);
  }
  for (const width of [0, NaN]) { const p = player(); p.layout(width); p.grant(75); p.release(); assert.deepEqual(p.seeks, []); }
});
test("unmounting during a hold cannot submit a seek", () => {
  const p = player(); p.layout(300); p.grant(75); p.unmount(); p.release(); assert.deepEqual(p.seeks, []);
});

test("both player layouts bind the shared gesture, preview and noninteractive fill", () => {
  for (const file of ["IOSPlayer.tsx", "RemotePlayer.tsx"]) {
    const source = fs.readFileSync(require("node:path").join(__dirname, "../src", file), "utf8");
    assert.match(source, /useProgressScrubber\(/);
    assert.match(source, /\{\.\.\.progressGesture\.panHandlers\}/);
    assert.match(source, /time\(progressGesture\.positionMs\)/);
    assert.match(source, /pointerEvents="none" style=\{(?:\[np\.fill|\{ height: 5)/);
    assert.doesNotMatch(source, /onTouchEnd=|progressStart|progressWidth/);
  }
});
