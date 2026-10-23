# iOS immersive UI publishing / build status

User approved Expo Go UI and requested push/package, requiring same UI in release except explicit development metadata mocks.

Published source: 1ac8a8360b3deba6f5f359a047222a453f73140d, followed by 64a989256c9ab18432c9bfd7d02ec5a902070e39 (transparent navigation scoped to immersive playback only). API publishing verified tree identity; unrelated working changes untouched. UI TypeScript/portable checks passed. Temporary image URL removed; mock guarded by __DEV__. No unused generated PNG assets committed.

run67 https://github.com/fengluoxiao/SDA/actions/runs/37816368618 failed twice at simulator scene screenshot, brightPixels0/glError1282. Audio smoke passed. run68 https://github.com/fengluoxiao/SDA/actions/runs/37826272891 also failed; scoping transparency did not resolve it. Do not claim known root cause, visual parity certification or device IPA success. No check bypassed. Both delivered artifact sets contain diagnostic logs, not an IPA. Previous run66 IPA remains old UI.

Delivery monitor state: tmp/ios-delivery/immersive-ui-2026-10-09/build-state.json. Diagnostics saved E:/SDA/artifacts/ios-immersive-ui-37816368618 and E:/SDA/artifacts/ios-immersive-ui-37826272891. Further scene renderer investigation required before packaging.


## Resolved / run70 delivered

Real screenshot showed visible geometry; default framebuffer readPixels was invalid. Explicit offscreen target for the same scene/camera returned brightPixels347/glError0; screenshot check retained. Subsequent failure was old accessibility label 音量滑块 vs new 音量; restored slider identity with no visual change. run70 succeeded, source5593ba2085c9f8085838c8bf989f2723662b4db7. IPA/digest/native bridge/new metadata strings verified, no Unsplash URL or development metadata mock strings in release. Details: [{"file": "E:\\SDA\\artifacts\\ios-immersive-ui-37865605904\\SDA-immersive-ui-SDK26-run70-unsigned.ipa", "sizeBytes": 16378675, "sha256": "b6441d3adfb023ab8e0d866732a8f112e67f72e5dc9caacc82c7bcdd19801334", "bundleId": "app.sda.mobile", "version": "0.1.0", "build": "1", "minimumOS": "16.4", "sdk": "iphoneos26.5", "architecture": "arm64", "signed": false, "enhancementUiAndNativeBridgePresent": true, "directHrtfManifestVerified": true, "sourceHead": "5593ba2085c9f8085838c8bf989f2723662b4db7", "runId": 37865605904}]
