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
  electron3D,
  rendererMode,
  getOutputLatencySeconds: () => ipcRenderer.sendSync("sda:get-output-latency-seconds"),
  setOutputLatencySeconds: (seconds) => ipcRenderer.sendSync("sda:set-output-latency-seconds", seconds),
  getVolumeBalanceEnabled: () => ipcRenderer.sendSync("sda:get-volume-balance-enabled"),
  setVolumeBalanceEnabled: (enabled) => ipcRenderer.sendSync("sda:set-volume-balance-enabled", enabled),
  getHeadTrackingStatus: () => ipcRenderer.invoke("sda:head-tracking-status"),
  startHeadTracking: () => ipcRenderer.invoke("sda:head-tracking-start"),
  stopHeadTracking: () => ipcRenderer.invoke("sda:head-tracking-stop"),
  recenterHeadTracking: () => ipcRenderer.invoke("sda:head-tracking-recenter"),
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
