/**
 * SDA desktop — Electron main process.
 *
 * The desktop app is the web build (apps/web/dist) plus:
 *  - native file open dialog / CLI file argument (no 4 GB File API limits —
 *    renderer reads via sda.readFileSlice IPC)
 *  - multichannel audio devices just work via Chromium (WASAPI exclusive
 *    would need a native output path; see docs for the plan)
 */

const { app, BrowserWindow, ipcMain, dialog } = require("electron");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { exec } = require("node:child_process");

/**
 * Windows 会把窗口被遮挡/最小化的进程打进 EcoQoS 效率模式，alt+tab、
 * 点任务栏、最小化其他窗口都会触发 —— AudioWorklet 跑在 renderer 进程里，
 * 被降速后 audio 回调来不及就是用户听到的"卡顿"。
 * 音频应用惯例：主进程 + 全部 Chromium 子进程提到 High（非 Realtime，安全）。
 * 音频服务进程在首次出声时才派生，所以每 30s 兜底重提一次。
 */
function boostProcessTreePriority() {
  if (process.platform !== "win32") return;
  try {
    os.setPriority(process.pid, os.constants.priority.PRIORITY_HIGH);
  } catch { /* 主进程提权失败不致命，继续兜底子进程 */ }
  const pid = process.pid;
  const script =
    `Get-CimInstance Win32_Process | Where-Object { $_.ParentProcessId -eq ${pid} } | ` +
    `ForEach-Object { try { (Get-Process -Id $_.ProcessId -ErrorAction Stop).PriorityClass = 'High' } catch {} }`;
  exec(`powershell -NoProfile -WindowStyle Hidden -Command "${script}"`, () => {});
}

const isDev = process.argv.includes("--dev");
// 默认硬件 GPU：3D 视图走显卡，不占 CPU（SwiftShader 软渲染会和音频解码抢 CPU，
// 外部应用一活动就容易供给抖动）。驱动有问题的机器可 SDA_ELECTRON_RENDERER=swiftshader 回退。
const requestedRenderer = process.env.SDA_ELECTRON_RENDERER ?? "hardware";
const rendererMode = ["swiftshader", "hardware", "2d"].includes(requestedRenderer)
  ? requestedRenderer
  : "hardware";
const enable3D = rendererMode !== "2d";
const openDevTools = process.env.SDA_OPEN_DEVTOOLS === "1" || process.argv.includes("--open-devtools");
const DEV_URL = process.env.SDA_DEV_URL ?? "http://localhost:5173";

// 音频应用：窗口被遮挡、最小化或切到后台时都不得节流 —
// Chromium 默认会冻结后台 renderer 的定时器/worker，直接导致解码喂不动 worklet。
app.commandLine.appendSwitch("disable-background-timer-throttling");
app.commandLine.appendSwitch("disable-backgrounding-occluded-windows");
app.commandLine.appendSwitch("disable-renderer-backgrounding");

// SwiftShader keeps the full WebGL/three.js scene without depending on the
// host GPU driver. Hardware mode is available for machines with stable drivers;
// 2d is an explicit emergency fallback only.
if (process.platform === "linux" && rendererMode === "swiftshader") {
  app.commandLine.appendSwitch("use-gl", "angle");
  app.commandLine.appendSwitch("use-angle", "swiftshader-webgl");
  app.commandLine.appendSwitch("enable-unsafe-swiftshader");
}
if (process.platform === "linux" && rendererMode === "2d") {
  app.commandLine.appendSwitch("disable-gpu");
  app.commandLine.appendSwitch("disable-gpu-compositing");
  app.commandLine.appendSwitch("disable-3d-apis");
  app.commandLine.appendSwitch("disable-software-rasterizer");
  app.disableHardwareAcceleration();
}

/** File handles the renderer has opened, id → path. */
const openFiles = new Map();
let nextFileId = 1;

const PROFILE_SCHEMA_VERSION = 1;
const BUNDLED_HEADPHONE_FIR_PATTERN = /^headphone-compensation\/[a-z0-9][a-z0-9-]*\/[A-Za-z0-9][A-Za-z0-9._-]*\.f32$/;
const BUNDLED_HRTF_PATTERN = /^hrtf\/(?:hrtf-set\.json|azm?\d+_elm?\d+_(?:dry|wet)\.f32)$/;
const profileStorePath = () => path.join(app.getPath("userData"), "headphone-compensation");

const OUTPUT_LATENCY_SECONDS = [0.1, 0.2, 0.3];
const DEFAULT_OUTPUT_LATENCY_SECONDS = 0.1;
const settingsPath = () => path.join(app.getPath("userData"), "settings.json");
const isOutputLatencySeconds = (value) => OUTPUT_LATENCY_SECONDS.includes(value);

function readSettings() {
  try {
    const settings = JSON.parse(fs.readFileSync(settingsPath(), "utf8"));
    return settings && typeof settings === "object" && !Array.isArray(settings) ? settings : {};
  } catch {
    return {};
  }
}

function writeSettings(updates) {
  fs.mkdirSync(app.getPath("userData"), { recursive: true });
  fs.writeFileSync(settingsPath(), JSON.stringify({ ...readSettings(), ...updates }), "utf8");
}

function readOutputLatencySeconds() {
  const { outputLatencySeconds } = readSettings();
  return isOutputLatencySeconds(outputLatencySeconds)
    ? outputLatencySeconds
    : DEFAULT_OUTPUT_LATENCY_SECONDS;
}

function writeOutputLatencySeconds(seconds) {
  if (!isOutputLatencySeconds(seconds)) throw new Error("invalid output latency seconds");
  writeSettings({ outputLatencySeconds: seconds });
}

function readVolumeBalanceEnabled() {
  return readSettings().volumeBalanceEnabled === true;
}

function writeVolumeBalanceEnabled(enabled) {
  if (typeof enabled !== "boolean") throw new Error("invalid volume balance setting");
  writeSettings({ volumeBalanceEnabled: enabled });
}

function readHeadTrackingEnabled() {
  return readSettings().experimentalHeadTrackingEnabled === true;
}

function writeHeadTrackingEnabled(enabled) {
  if (typeof enabled !== "boolean") throw new Error("invalid head tracking setting");
  writeSettings({ experimentalHeadTrackingEnabled: enabled });
}

function readLastMediaDirectory() {
  const directory = readSettings().lastMediaDirectory;
  try {
    return typeof directory === "string" && fs.statSync(directory).isDirectory() ? directory : null;
  } catch {
    return null;
  }
}

// Phase 2 experimental provider. This is deliberately a calibrated mock only:
// Windows AirPods transport remains an external user-mode helper defined in docs,
// not a bundled Bluetooth implementation or driver.
const HEAD_TRACKING_MOCK_INTERVAL_MS = 20;
let headTrackingTimer = null;
let headTrackingStartedAt = 0;
let headTrackingStatus = { running: false, source: "mock", detail: "未启动" };

function sendHeadTracking(channel, payload) {
  for (const win of BrowserWindow.getAllWindows()) {
    if (!win.isDestroyed()) win.webContents.send(channel, payload);
  }
}

function publishHeadTrackingStatus() {
  sendHeadTracking("sda:head-tracking-status", headTrackingStatus);
  return headTrackingStatus;
}

function mockHeadPose() {
  const elapsedSeconds = (Date.now() - headTrackingStartedAt) / 1000;
  // Gentle yaw-only motion. The quaternion is normalized and uses SDA's
  // world-coordinate convention; it exercises renderer APIs without hardware.
  const yawRadians = Math.sin(elapsedSeconds * 0.7) * (Math.PI / 6);
  return {
    timestampMs: Date.now(),
    orientation: { x: 0, y: 0, z: Math.sin(yawRadians / 2), w: Math.cos(yawRadians / 2) },
  };
}

function startHeadTracking() {
  if (headTrackingTimer) return publishHeadTrackingStatus();
  writeHeadTrackingEnabled(true);
  headTrackingStartedAt = Date.now();
  headTrackingStatus = { running: true, source: "mock", detail: "模拟 yaw 姿态（仅实验）" };
  headTrackingTimer = setInterval(() => sendHeadTracking("sda:head-tracking-pose", mockHeadPose()), HEAD_TRACKING_MOCK_INTERVAL_MS);
  headTrackingTimer.unref();
  return publishHeadTrackingStatus();
}

function stopHeadTracking(persist = true) {
  if (headTrackingTimer) clearInterval(headTrackingTimer);
  headTrackingTimer = null;
  if (persist) writeHeadTrackingEnabled(false);
  headTrackingStatus = { running: false, source: "mock", detail: "已停止" };
  return publishHeadTrackingStatus();
}

function recenterHeadTracking() {
  if (!headTrackingTimer) throw new Error("head tracking is not running");
  headTrackingStartedAt = Date.now();
  const pose = mockHeadPose();
  sendHeadTracking("sda:head-tracking-recenter", pose);
  return pose;
}

const webAssetRoots = () => [
  path.join(__dirname, "web"),
  path.join(__dirname, "../web/dist"),
];
const webAssetRoot = () => webAssetRoots().find((candidate) => fs.existsSync(path.join(candidate, "index.html")));
const sha256 = (bytes) => crypto.createHash("sha256").update(bytes).digest("hex");

function profileValidationError(message) {
  throw new Error(`耳机校准档案无效: ${message}`);
}

function safeProfileAsset(asset, side) {
  if (!asset || typeof asset !== "object") profileValidationError(`缺少 ${side} FIR`);
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]*\.f32$/.test(asset.fileName ?? "")) profileValidationError(`${side} FIR 文件名无效`);
  if (!Number.isInteger(asset.tapCount) || asset.tapCount < 2) profileValidationError(`${side} FIR tapCount 无效`);
  if (!/^[a-f0-9]{64}$/i.test(asset.sha256 ?? "")) profileValidationError(`${side} FIR SHA-256 无效`);
  return asset;
}

function validateProfilePackage(manifest, directory) {
  if (manifest?.schemaVersion !== PROFILE_SCHEMA_VERSION) profileValidationError("schemaVersion 必须为 1");
  if (!/^[a-z0-9][a-z0-9-]*$/.test(manifest?.id ?? "")) profileValidationError("id 必须是小写 slug");
  if (!["independent-lr", "average-dual-mono"].includes(manifest.measurementMode)) profileValidationError("measurementMode 无效");
  for (const key of ["name", "source", "target", "channelClaim", "createdAt", "deviceRevision", "playbackState", "earTips", "firmware", "measurementRig", "referenceBand"]) {
    if (typeof manifest[key] !== "string" || !manifest[key].trim()) profileValidationError(`缺少 ${key}`);
  }
  if (manifest.measurementMode === "independent-lr") {
    for (const key of ["leftMeasurement", "rightMeasurement", "balanceEvidence"]) {
      if (typeof manifest[key] !== "string" || !manifest[key].trim()) profileValidationError(`独立 L/R profile 缺少 ${key}`);
    }
  } else {
    for (const key of ["averageMeasurement", "derivation"]) {
      if (typeof manifest[key] !== "string" || !manifest[key].trim()) profileValidationError(`平均双单声道 profile 缺少 ${key}`);
    }
    if (!/not independent|非独立|同一.*(?:eq|曲线)/i.test(manifest.channelClaim)) profileValidationError("平均双单声道 profile 必须声明非独立 L/R");
  }
  if (!Number.isFinite(Date.parse(manifest.createdAt))) profileValidationError("createdAt 无效");
  if (!Number.isFinite(manifest.sampleRate) || manifest.sampleRate <= 0) profileValidationError("sampleRate 无效");
  if (!Number.isFinite(manifest.preampDb) || manifest.preampDb > 0) profileValidationError("preampDb 必须是有限非正值");
  const left = safeProfileAsset(manifest.leftFir, "left");
  const right = safeProfileAsset(manifest.rightFir, "right");
  const sharedAsset = left.fileName === right.fileName || left.sha256 === right.sha256;
  if (manifest.measurementMode === "independent-lr" && sharedAsset) profileValidationError("独立 L/R profile 的左右 FIR 必须是独立资产");
  if (manifest.measurementMode === "average-dual-mono" && !sharedAsset) profileValidationError("平均双单声道 profile 的左右 FIR 必须是同一资产");
  const readAsset = (asset, side) => {
    const filePath = path.resolve(directory, asset.fileName);
    if (path.dirname(filePath) !== path.resolve(directory)) profileValidationError(`${side} FIR 路径越界`);
    const bytes = fs.readFileSync(filePath);
    if (bytes.byteLength !== asset.tapCount * Float32Array.BYTES_PER_ELEMENT || !bytes.byteLength) profileValidationError(`${side} FIR 长度不符`);
    const taps = new Float32Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / Float32Array.BYTES_PER_ELEMENT);
    if (![...taps].every(Number.isFinite)) profileValidationError(`${side} FIR 含无效 tap`);
    if (sha256(bytes) !== asset.sha256.toLowerCase()) profileValidationError(`${side} FIR SHA-256 不匹配`);
    return bytes;
  };
  return { manifest, leftFir: readAsset(left, "left"), rightFir: readAsset(right, "right") };
}

function readStoredProfile(id) {
  if (!/^[a-z0-9][a-z0-9-]*$/.test(id ?? "")) profileValidationError("profile id 无效");
  const directory = path.join(profileStorePath(), id);
  return validateProfilePackage(JSON.parse(fs.readFileSync(path.join(directory, "profile.json"), "utf8")), directory);
}

function createWindow() {
  const win = new BrowserWindow({
    width: 1440,
    height: 900,
    backgroundColor: "#0c101c",
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      // Keep the renderer active when the window is covered or detached.
      backgroundThrottling: false,
      additionalArguments: [`--sda-electron-renderer=${rendererMode}`],
    },
  });

  win.webContents.on("did-fail-load", (_event, errorCode, errorDescription, validatedURL) => {
    console.error(`[SDA] 页面加载失败 ${errorCode} ${errorDescription}: ${validatedURL}`);
    dialog.showErrorBox("SDA 页面加载失败", `${errorDescription}\n${validatedURL}`);
  });
  win.webContents.on("render-process-gone", (_event, details) => {
    console.error(`[SDA] renderer 退出: ${details.reason}${details.exitCode ? ` (${details.exitCode})` : ""}`);
    if (details.reason !== "clean-exit") {
      dialog.showErrorBox(
        "SDA 3D 渲染进程失败",
        `WebGL 渲染进程异常退出（${details.reason}）。可用 SDA_ELECTRON_RENDERER=2d 临时启动。`,
      );
    }
  });
  win.webContents.on("console-message", (_event, level, message, line, sourceId) => {
    if (level >= 2) console.warn(`[SDA renderer] ${sourceId}:${line} ${message}`);
    else if (message.startsWith("[SDA]")) console.log(message);
  });

  if (isDev) {
    win.loadURL(DEV_URL);
    if (openDevTools) win.webContents.openDevTools({ mode: "detach" });
  } else {
    const root = webAssetRoot();
    const entry = root ? path.join(root, "index.html") : null;
    if (!entry) {
      dialog.showErrorBox(
        "SDA 无法启动",
        "找不到网页资源。请先运行 pnpm web:build，或使用 --dev 启动开发服务器。",
      );
      app.quit();
      return;
    }
    win.loadFile(entry);
  }

  // `sda --dev movie.mkv` or double-clicked file association.
  const fileArg = process.argv.find(
    (a, i) => i > 1 && !a.startsWith("-") && fs.existsSync(a) && !a.endsWith(".cjs"),
  );
  if (fileArg) {
    win.webContents.once("did-finish-load", () => {
      win.webContents.send("sda:open-file", fileArg);
    });
  }
}

ipcMain.on("sda:get-output-latency-seconds", (event) => {
  event.returnValue = readOutputLatencySeconds();
});

ipcMain.on("sda:set-output-latency-seconds", (event, seconds) => {
  try {
    writeOutputLatencySeconds(seconds);
    event.returnValue = true;
  } catch (error) {
    console.warn("[SDA] 输出延迟设置未保存:", error);
    event.returnValue = false;
  }
});

ipcMain.on("sda:get-volume-balance-enabled", (event) => {
  event.returnValue = readVolumeBalanceEnabled();
});

ipcMain.on("sda:set-volume-balance-enabled", (event, enabled) => {
  try {
    writeVolumeBalanceEnabled(enabled);
    event.returnValue = true;
  } catch (error) {
    console.warn("[SDA] 音量平衡设置未保存:", error);
    event.returnValue = false;
  }
});

ipcMain.handle("sda:head-tracking-status", () => headTrackingStatus);
ipcMain.handle("sda:head-tracking-start", () => startHeadTracking());
ipcMain.handle("sda:head-tracking-stop", () => stopHeadTracking());
ipcMain.handle("sda:head-tracking-recenter", () => recenterHeadTracking());

ipcMain.handle("sda:pick-file", async (event) => {
  // 挂到发起窗口上：弹窗跟随主窗口置顶，不会跑到后台/其他显示器；
  // 且异步版本不会冻结主进程事件循环，播放中的 IPC 读文件不受影响。
  const parent = BrowserWindow.fromWebContents(event.sender);
  const defaultPath = readLastMediaDirectory();
  const options = {
    ...(defaultPath ? { defaultPath } : {}),
    filters: [
      { name: "Audio / Video", extensions: ["mkv", "mka", "mp4", "m4a", "wav", "bwf", "rf64", "thd", "mlp", "ec3", "eac3", "ac3", "dts"] },
      { name: "All Files", extensions: ["*"] },
    ],
    properties: ["openFile"],
  };
  const { canceled, filePaths } = parent
    ? await dialog.showOpenDialog(parent, options)
    : await dialog.showOpenDialog(options);
  const filePath = canceled ? null : filePaths[0] ?? null;
  if (filePath) {
    try {
      writeSettings({ lastMediaDirectory: path.dirname(filePath) });
    } catch (error) {
      console.warn("[SDA] 最近媒体目录未保存:", error);
    }
  }
  return filePath;
});

ipcMain.handle("sda:open-path", (_e, filePath) => {
  const stat = fs.statSync(filePath);
  const id = nextFileId++;
  openFiles.set(id, filePath);
  return { id, size: stat.size, name: path.basename(filePath) };
});

ipcMain.handle("sda:read-slice", (_e, id, offset, length) => {
  const filePath = openFiles.get(id);
  if (!filePath) throw new Error("unknown file id");
  const fd = fs.openSync(filePath, "r");
  try {
    const buf = Buffer.alloc(length);
    const read = fs.readSync(fd, buf, 0, length, offset);
    return buf.subarray(0, read);
  } finally {
    fs.closeSync(fd);
  }
});

ipcMain.handle("sda:close", (_e, id) => {
  openFiles.delete(id);
});

ipcMain.handle("sda:read-bundled-headphone-fir", (_e, assetPath) => {
  if (typeof assetPath !== "string" || !BUNDLED_HEADPHONE_FIR_PATTERN.test(assetPath)) {
    profileValidationError("内置 FIR 路径无效");
  }
  const root = webAssetRoot();
  if (!root) throw new Error("找不到内置耳机补偿资产目录");
  const resolvedRoot = path.resolve(root);
  const filePath = path.resolve(root, ...assetPath.split("/"));
  if (path.dirname(path.dirname(filePath)) !== path.join(resolvedRoot, "headphone-compensation")) {
    profileValidationError("内置 FIR 路径越界");
  }
  return fs.readFileSync(filePath);
});

ipcMain.handle("sda:read-bundled-hrtf", (_e, assetPath) => {
  if (typeof assetPath !== "string" || !BUNDLED_HRTF_PATTERN.test(assetPath)) {
    throw new Error("内置 HRTF 路径无效");
  }
  const root = webAssetRoot();
  if (!root) throw new Error("找不到内置 HRTF 资产目录");
  const hrtfRoot = path.resolve(root, "hrtf");
  const filePath = path.resolve(root, ...assetPath.split("/"));
  if (path.dirname(filePath) !== hrtfRoot) throw new Error("内置 HRTF 路径越界");
  if (assetPath.endsWith("hrtf-set.json")) console.log("[SDA] 从 Electron 内置资源加载 HRTF");
  return fs.readFileSync(filePath);
});

ipcMain.handle("sda:import-headphone-profile", async (event) => {
  const parent = BrowserWindow.fromWebContents(event.sender);
  const options = {
    title: "选择耳机校准档案 profile.json",
    filters: [{ name: "Headphone profile", extensions: ["json"] }],
    properties: ["openFile"],
  };
  const { canceled, filePaths } = parent
    ? await dialog.showOpenDialog(parent, options)
    : await dialog.showOpenDialog(options);
  if (canceled) return null;
  const sourceManifest = path.resolve(filePaths[0]);
  const sourceDirectory = path.dirname(sourceManifest);
  const manifest = JSON.parse(fs.readFileSync(sourceManifest, "utf8"));
  const verified = validateProfilePackage(manifest, sourceDirectory);
  const target = path.join(profileStorePath(), manifest.id);
  fs.rmSync(target, { recursive: true, force: true });
  fs.mkdirSync(target, { recursive: true });
  fs.copyFileSync(sourceManifest, path.join(target, "profile.json"));
  fs.writeFileSync(path.join(target, manifest.leftFir.fileName), verified.leftFir);
  if (manifest.rightFir.fileName !== manifest.leftFir.fileName) {
    fs.writeFileSync(path.join(target, manifest.rightFir.fileName), verified.rightFir);
  }
  const stored = readStoredProfile(manifest.id);
  return { profile: stored.manifest, leftFir: stored.leftFir, rightFir: stored.rightFir };
});

ipcMain.handle("sda:list-headphone-profiles", () => {
  const root = profileStorePath();
  if (!fs.existsSync(root)) return [];
  return fs.readdirSync(root, { withFileTypes: true }).flatMap((entry) => {
    if (!entry.isDirectory()) return [];
    try {
      return [readStoredProfile(entry.name).manifest];
    } catch (error) {
      console.warn(`[SDA] 忽略无效耳机 profile ${entry.name}:`, error);
      return [];
    }
  });
});

ipcMain.handle("sda:read-headphone-profile", (_e, id) => {
  const stored = readStoredProfile(id);
  return { profile: stored.manifest, leftFir: stored.leftFir, rightFir: stored.rightFir };
});

ipcMain.handle("sda:delete-headphone-profile", (_e, id) => {
  if (!/^[a-z0-9][a-z0-9-]*$/.test(id ?? "")) profileValidationError("profile id 无效");
  fs.rmSync(path.join(profileStorePath(), id), { recursive: true, force: true });
});

app.whenReady().then(() => {
  boostProcessTreePriority();
  setInterval(boostProcessTreePriority, 30_000).unref();
  createWindow();
  if (readHeadTrackingEnabled()) startHeadTracking();
  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", () => {
  stopHeadTracking(false);
  if (process.platform !== "darwin") app.quit();
});
