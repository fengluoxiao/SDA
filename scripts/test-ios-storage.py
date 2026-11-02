#!/usr/bin/env python3
"""Execute the real Swift cleanup against isolated test folders (macOS/Xcode)."""
from pathlib import Path
import subprocess,tempfile
root=Path(__file__).resolve().parents[1]
policy=(root/'apps/mobile/modules/sda-core/ios/PlaybackStorage.swift').read_text(encoding='utf-8')
tests=r'''
enum SdaError: Error { case message(String) }
let root=FileManager.default.temporaryDirectory.appendingPathComponent("sda-storage-test-"+UUID().uuidString)
let cache=root.appendingPathComponent("cache"),outside=root.appendingPathComponent("outside")
try FileManager.default.createDirectory(at:cache,withIntermediateDirectories:true)
try FileManager.default.createDirectory(at:outside,withIntermediateDirectories:true)
defer { try? FileManager.default.removeItem(at:root) }
func file(_ url:URL,_ text:String="audio") throws { try Data(text.utf8).write(to:url) }
func exists(_ url:URL)->Bool { FileManager.default.fileExists(atPath:url.path) }
let original=outside.appendingPathComponent("original.wav");try file(original)
let extracted=cache.appendingPathComponent("sda-import-00000000-0000-0000-0000-000000000001.mhas");try file(extracted)
let legacy=cache.appendingPathComponent("00000000-0000-0000-0000-000000000002.mhas");try file(legacy)
let queued=cache.appendingPathComponent("00000000-0000-0000-0000-000000000003.mhas");try file(queued)
let unrelated=cache.appendingPathComponent("user-recording.mhas");try file(unrelated)
let room=cache.appendingPathComponent("sda-format-123-1")
try FileManager.default.createDirectory(at:room,withIntermediateDirectories:true);try file(room.appendingPathComponent("hrtf.f32"))
let link=cache.appendingPathComponent("sda-format-123-2")
try FileManager.default.createSymbolicLink(at:link,withDestinationURL:outside)
let nested=cache.appendingPathComponent("sda-format-123-3")
try FileManager.default.createDirectory(at:nested,withIntermediateDirectories:true)
try FileManager.default.createSymbolicLink(at:nested.appendingPathComponent("link"),withDestinationURL:outside)
let picker=cache.appendingPathComponent("DocumentPicker")
try FileManager.default.createDirectory(at:picker,withIntermediateDirectories:true)
let legacySong=picker.appendingPathComponent("library-song.m4a");try file(legacySong)
PlaybackStorage.cleanAbandoned([cache],protected:[queued.resolvingSymlinksInPath().standardizedFileURL.path])
assert(!exists(extracted) && !exists(legacy) && !exists(room))
assert(exists(original) && exists(queued) && exists(unrelated) && exists(legacySong))
assert(exists(link) && exists(nested))
let text=try String(contentsOf:original,encoding:.utf8);assert(text=="audio")
let duplicate=picker.appendingPathComponent("duplicate.m4a");try file(duplicate)
let unique=picker.appendingPathComponent("unique.m4a");try file(unique,"different song")
assert(PlaybackStorage.deduplicatePicker(picker,candidates:[legacySong,duplicate,unique],protected:[legacySong.resolvingSymlinksInPath().standardizedFileURL.path]))
assert(exists(legacySong) && exists(unique) && !exists(duplicate))
print("iOS storage: owned temporary/resource cleanup, queue and legacy-song protection, symlink boundaries passed")
'''
with tempfile.TemporaryDirectory(prefix='sda-ios-storage-') as temporary:
 script=Path(temporary)/'main.swift';script.write_text(policy+tests,encoding='utf-8')
 subprocess.run(['xcrun','swift',str(script)],check=True)
