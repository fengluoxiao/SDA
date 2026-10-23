// Source-contract regression; macOS Swift compile/device interruption checks
// remain necessary. These assertions do not simulate AVAudioSession behavior.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const source = readFileSync('apps/mobile/modules/sda-core/ios/SdaPlayer.swift', 'utf8');
assert.match(source, /error.domain == NSOSStatusErrorDomain && \[560557684, 561017449\].contains\(error.code\)/);
const activation = source.slice(source.indexOf('func activatePlaybackSession'), source.indexOf('var inBackground'));
assert.match(activation, /guard Self.isTemporarySessionActivationError.*else \{ throw error \}/);
assert.match(activation, /sessionActivationPending = true/);
assert.doesNotMatch(activation, /while|sleep|asyncAfter/);
const startup = source.slice(source.indexOf('func startNative'), source.indexOf('func setPaused'));
assert.match(startup, /if sessionReady \{ try engine.start\(\) \}/);
assert.match(startup, /if !sessionReady.*command\("pause"/);
const pause = source.slice(source.indexOf('func setPaused'), source.indexOf('func updateNowPlaying'));
assert.match(pause, /if paused \{\s*sessionActivationPending = false/);
assert.match(pause, /guard try activatePlaybackSession\(\) else/);
assert.match(pause, /audio\?\.pause\(\); isPaused = true/);
const interruption = source.slice(source.indexOf('AVAudioSession.interruptionNotification'), source.indexOf('AVAudioSession.routeChangeNotification'));
assert.match(interruption, /if shouldResume \{ _ = try\? self.setPaused\(false\) \}/);
assert.doesNotMatch(interruption, /if shouldResume \|\|/);
const stop = source.slice(source.indexOf('func stopNative'), source.indexOf('func setAlacStereoUpmix'));
assert.match(stop, /sessionActivationPending = false/);
assert.match(source, /if !sessionActivationPending \{ beginPreparation\(token\) \}/);
console.log('iOS session recovery source contracts passed; Swift/device validation still required.');
assert.match(source, /let lead = consumed == 0 \? 4\*48000 : \(alacActive \? 24000 : 4\*48000\)/);
