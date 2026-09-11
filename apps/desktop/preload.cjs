/** Preload: expose the minimal file-access bridge to the web build. */
const { contextBridge, ipcRenderer } = require("electron");

const rendererModeArg = process.argv.find((arg) => arg.startsWith("--sda-electron-renderer="));
const rendererMode = rendererModeArg?.split("=", 2)[1] ?? "swiftshader";
const electron3D = rendererMode !== "2d";

const pendingOpenPaths = [];
let openFileCallback = null;
ipcRenderer.on("sda:open-file", (_event, filePath) => {
  if (openFileCallback) openFileCallback(filePath);
  else pendingOpenPaths.push(filePath);
});

function subscribe(channel, callback) {
  const listener = (_event, value) => callback(value);
  ipcRenderer.on(channel, listener);
  return () => ipcRenderer.removeListener(channel, listener);
}

contextBridge.exposeInMainWorld("sdaDesktop", {
  getRemotePairingKey: () => ipcRenderer.invoke("sda:remote-pairing-key"),
  getRemoteStatus: () => ipcRenderer.invoke("sda:remote-status"),
  remoteSession: (action, value) => ipcRenderer.invoke("sda:remote-session", action, value),
  nativeRendererEnd:(sample)=>ipcRenderer.invoke('sda:native-renderer-end',sample),
  remoteCommand: command => ipcRenderer.invoke("sda:remote-command", command),
  publishRemoteScene: scene => ipcRenderer.send("sda:remote-scene", scene),
  publishRemoteState: state => ipcRenderer.send("sda:remote-state", state),
  completeRemoteControl: (id, error) => ipcRenderer.send("sda:remote-complete", id, error),
  onRemoteStatus: callback => subscribe("sda:remote-status", callback),
  onRemoteControl: callback => subscribe("sda:remote-control", callback),
  onRemoteSuspend: callback => subscribe("sda:remote-suspend", callback),
  onRemoteResult: callback => subscribe("sda:remote-result", callback),
  electron3D,
  browseMedia: (action, value) => ipcRenderer.invoke("sda:media-browser", action, value),
  browsePersonalHrtf: (action, value) => ipcRenderer.invoke("sda:personal-hrtf-browser", action, value),
  listPersonalHrtf: () => ipcRenderer.invoke("sda:list-personal-hrtf"),
  renamePersonalHrtf: (id,name) => ipcRenderer.invoke("sda:rename-personal-hrtf",id,name),
  personalHrtfArchive: (action,id,value) => ipcRenderer.invoke("sda:personal-hrtf-archive",action,id,value),
  generatePersonalHrtf: (parameters, assessment) => ipcRenderer.invoke("sda:generate-personal-hrtf", parameters, assessment),
  importPersonalHrtf: (sourcePath) => ipcRenderer.invoke("sda:import-personal-hrtf", sourcePath),
  windowControl: action => ipcRenderer.invoke("sda:window-control", action),
  getWindowMaximized: () => ipcRenderer.invoke("sda:window-state"),
  onWindowMaximized: callback => subscribe("sda:window-state", callback),
  rendererMode,
  getOutputLatencySeconds: () => ipcRenderer.sendSync("sda:get-output-latency-seconds"),
  setOutputLatencySeconds: (seconds) => ipcRenderer.sendSync("sda:set-output-latency-seconds", seconds),
  getVolumeBalanceEnabled: () => ipcRenderer.sendSync("sda:get-volume-balance-enabled"),
  setVolumeBalanceEnabled: (enabled) => ipcRenderer.sendSync("sda:set-volume-balance-enabled", enabled),
  getHeadTrackingStatus: () => ipcRenderer.invoke("sda:head-tracking-status"),
  getHeadTrackingHelper: () => ipcRenderer.invoke("sda:head-tracking-helper"),
  selectHeadTrackingHelper: () => ipcRenderer.invoke("sda:head-tracking-select-helper"),
  useBundledHeadTrackingHelper: () => ipcRenderer.invoke("sda:head-tracking-use-bundled-helper"),
  startHeadTracking: () => ipcRenderer.invoke("sda:head-tracking-start"),
  stopHeadTracking: () => ipcRenderer.invoke("sda:head-tracking-stop"),
  recenterHeadTracking: () => ipcRenderer.invoke("sda:head-tracking-recenter"),
  getNativeRendererStatus: () => ipcRenderer.invoke("sda:native-renderer-status"),
  getOutputDevices: () => ipcRenderer.invoke("sda:output-devices"),
  setOutputDevice: value => ipcRenderer.invoke("sda:set-output-device", value),
  onOutputDevices: callback => subscribe("sda:output-devices", callback),
  startNativeRenderer: () => ipcRenderer.invoke("sda:native-renderer-start"),
  stopNativeRenderer: () => ipcRenderer.invoke("sda:native-renderer-stop"),
  getNativeRendererHealth: () => ipcRenderer.invoke("sda:native-renderer-health"),
  nativeRendererSource: (source) => ipcRenderer.invoke("sda:native-renderer-source", source),
  nativeRendererRemoveSource: (id, atSample) => ipcRenderer.invoke("sda:native-renderer-remove-source", id, atSample),
  nativeRendererEvents: (events) => ipcRenderer.invoke("sda:native-renderer-events", events),
  nativeRendererReset: (origin) => ipcRenderer.invoke("sda:native-renderer-reset", origin),
  nativeRendererMuted: (id, muted, atSample) => ipcRenderer.invoke("sda:native-renderer-muted", id, muted, atSample),
  nativeRendererLfeMuted: (muted) => ipcRenderer.invoke("sda:native-renderer-lfe-muted", muted),
  nativeRendererSpeakerMutes: (names, focus) => ipcRenderer.invoke("sda:native-renderer-speaker-mutes", names, focus),
  nativeRendererVolume: (volume) => ipcRenderer.invoke("sda:native-renderer-volume", volume),
  nativeRendererProgramEnabled: (enabled) => ipcRenderer.invoke("sda:native-renderer-program-enabled", enabled),
  nativeRendererProgramGain: (gain, atSample) => ipcRenderer.invoke("sda:native-renderer-program-gain", gain, atSample),
  nativeRendererBinauralEq: (bands, lowCut) => ipcRenderer.invoke("sda:native-renderer-binaural-eq", bands, lowCut),
  nativeRendererHeadphoneProfile: (id, source) => ipcRenderer.invoke("sda:native-renderer-headphone-profile", id, source),
  nativeRendererPose: (orientation) => ipcRenderer.invoke("sda:native-renderer-pose", orientation),
  nativeRendererClearPose: () => ipcRenderer.invoke("sda:native-renderer-clear-pose"),
  nativeRendererHrtf: (set, wetWeight) => ipcRenderer.invoke("sda:native-renderer-hrtf", set, wetWeight),
  nativeRendererStereoMode: (mode) => ipcRenderer.invoke("sda:native-renderer-stereo-mode", mode),
  getCinemaSettings: () => ipcRenderer.invoke("sda:cinema-settings"),
  listCinemaRooms: () => ipcRenderer.invoke("sda:cinema-rooms"),
  importCinemaRoom: () => ipcRenderer.invoke("sda:cinema-import"),
  deleteCinemaRoom: (id) => ipcRenderer.invoke("sda:cinema-delete", id),
  exportCinemaReport: (id) => ipcRenderer.invoke("sda:cinema-export-report", id),
  nativeRendererCinema: (settings, profileId) => ipcRenderer.invoke("sda:native-renderer-cinema", settings, profileId),
  roomLabStatus: () => ipcRenderer.invoke("sda:room-lab-status"),
  roomLabGenerate: (config) => ipcRenderer.invoke("sda:room-lab-generate", config),
  roomLabCancel: () => ipcRenderer.invoke("sda:room-lab-cancel"),
  nativeRendererComparisonGain: (gainDb) => ipcRenderer.invoke("sda:comparison-gain",gainDb),
  nativeRendererObjectHrtf: (enabled) => ipcRenderer.invoke("sda:native-renderer-object-hrtf", enabled),
  nativeRendererDirectionalHrtf: (enabled) => ipcRenderer.invoke("sda:native-renderer-directional-hrtf", enabled),
  nativeRendererNearField: (settings) => ipcRenderer.invoke("sda:native-renderer-near-field", settings),
  nativeRendererSourceExtent: (settings) => ipcRenderer.invoke("sda:native-renderer-source-extent", settings),
  nativeRendererLayout: (layout) => ipcRenderer.invoke("sda:native-renderer-layout", layout),
  nativeRendererOutputActive: (active) => ipcRenderer.invoke("sda:native-renderer-output-active", active),
  nativeRendererStartAt: (origin) => ipcRenderer.invoke("sda:native-renderer-start-at", origin),
  nativeRendererPause: (paused) => ipcRenderer.invoke("sda:native-renderer-pause", paused),
  nativeRendererFrame: (samplePos, entries, events) => ipcRenderer.invoke("sda:native-renderer-frame", samplePos, entries, events),
  onNativeRendererStatus: (callback) => subscribe("sda:native-renderer-status", callback),
  onNativeRendererObjectActivity: (callback) => subscribe("sda:native-renderer-object-activity", callback),
  onHeadTrackingStatus: (callback) => subscribe("sda:head-tracking-status", callback),
  onHeadTrackingPose: (callback) => subscribe("sda:head-tracking-pose", callback),
  onHeadTrackingRecenter: (callback) => subscribe("sda:head-tracking-recenter", callback),
  pickFile: () => ipcRenderer.invoke("sda:pick-file"),
  pickFolder: () => ipcRenderer.invoke("sda:pick-folder"),
  openPath: (filePath) => ipcRenderer.invoke("sda:open-path", filePath),
  readSlice: (id, offset, length) => ipcRenderer.invoke("sda:read-slice", id, offset, length),
  close: (id) => ipcRenderer.invoke("sda:close", id),
  readBundledHeadphoneFir: (assetPath) => ipcRenderer.invoke("sda:read-bundled-headphone-fir", assetPath),
  readBundledHrtf: (assetPath) => ipcRenderer.invoke("sda:read-bundled-hrtf", assetPath),
  importHeadphoneProfile: () => ipcRenderer.invoke("sda:import-headphone-profile"),
  listHeadphoneProfiles: () => ipcRenderer.invoke("sda:list-headphone-profiles"),
  readHeadphoneProfile: (id) => ipcRenderer.invoke("sda:read-headphone-profile", id),
  deleteHeadphoneProfile: (id) => ipcRenderer.invoke("sda:delete-headphone-profile", id),
  onOpenFile: (callback) => {
    openFileCallback = callback;
    for (const filePath of pendingOpenPaths.splice(0)) callback(filePath);
    return () => {
      if (openFileCallback === callback) openFileCallback = null;
    };
  },
});
