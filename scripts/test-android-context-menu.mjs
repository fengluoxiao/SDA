import assert from 'node:assert/strict';
import { build } from 'esbuild';

// Exercise the real adapter at the native event boundary without mounting RN.
const compiled = await build({
  entryPoints: ['apps/mobile/src/AndroidGlassSettingsButton.tsx'],
  bundle: true, platform: 'node', format: 'cjs', write: false,
  external: ['react', 'react/jsx-runtime', 'react-native', 'expo'],
});
function load({ native = true, os = 'android', dark = false } = {}) {
  const module = { exports: {} };
  const element = (type, props) => ({ type, props });
  const dependencies = {
    react: { createElement: element },
    'react/jsx-runtime': { jsx: element, jsxs: element },
    'react-native': { Platform: { OS: os }, useColorScheme: () => dark ? 'dark' : 'light' },
    expo: { requireNativeView: name => name },
  };
  new Function('require', 'module', 'exports', 'globalThis', compiled.outputFiles[0].text)(
    name => { assert.ok(name in dependencies, name); return dependencies[name]; },
    module, module.exports, { expo: { modules: native ? { SdaAndroidChrome: {} } : {} } },
  );
  return module.exports.AndroidGlassSettingsButton;
}
for (const dark of [false, true]) for (const active of [false, true]) {
  const Button = load({ dark });
  const events = [];
  const node = Button({ spatial: true, spatialEnabled: active, onSpatialSelect: value => events.push(value) });
  assert.equal(node.props.dark, dark);
  assert.equal(node.props.glassDark, dark);
  assert.equal(node.props.spatialEnabled, active);
  assert.equal(node.props.style.marginRight, 8);
  node.props.onSpatialSelect({ nativeEvent: { enabled: false } });
  node.props.onSpatialSelect({ nativeEvent: { enabled: true } });
  for (const invalid of [null, undefined, 0, 1, 'true', 'false']) {
    node.props.onSpatialSelect({ nativeEvent: { enabled: invalid } });
  }
  assert.deepEqual(events, [false, true], 'native boolean selection must arrive without coercion');
  const busy = Button({ spatial: true, disabled: true, onSpatialSelect: value => events.push(value) });
  busy.props.onSpatialSelect({ nativeEvent: { enabled: true } });
  assert.deepEqual(events, [false, true], 'late native events must not change busy player state');
  let settings = 0;
  Button({ onPress: () => settings++ }).props.onExpand();
  assert.equal(settings, 1, 'settings/back action still dispatches independently');
  assert.equal(Button({ spatial: true, sideDock: true }).props.style.marginRight, 0);
}
assert.equal(load({ native: false })({}), null);
assert.equal(load({ os: 'ios' })({}), null);
console.log('Android context menu native event routing: Off/Fixed, controlled selection, disabled/stale events, appearance, rail and settings passed.');
