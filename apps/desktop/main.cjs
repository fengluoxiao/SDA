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
const { exec, spawn } = require("node:child_process");

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
const MEDIA_EXTENSIONS = new Set([".mkv", ".mka", ".mp4", ".m4a", ".wav", ".bwf", ".rf64", ".thd", ".mlp", ".ec3", ".eac3", ".ac3", ".dts"]);
const MEDIA_DIALOG_EXTENSIONS = [...MEDIA_EXTENSIONS].map((extension) => extension.slice(1));
const MAX_FOLDER_MEDIA_FILES = 2000;
const MAX_FOLDER_ENTRIES = 20000;
const MAX_SLICE_BYTES = 1 << 20;

function isMediaFile(filePath) {
  return MEDIA_EXTENSIONS.has(path.extname(filePath).toLowerCase());
}

function scanMediaFolder(root) {
  const resolvedRoot = fs.realpathSync(root);
  if (!fs.statSync(resolvedRoot).isDirectory()) throw new Error("选择的路径不是文件夹");
  const paths = [];
  let visitedEntries = 0;
  const walk = (directory) => {
    let entries;
    try {
      entries = fs.readdirSync(directory, { withFileTypes: true });
    } catch (error) {
      console.warn("[SDA] 跳过不可读取目录:", directory, error);
      return;
    }
    entries.sort((a, b) => a.name.localeCompare(b.name));
    for (const entry of entries) {
      if (++visitedEntries > MAX_FOLDER_ENTRIES || paths.length >= MAX_FOLDER_MEDIA_FILES) return;
      if (entry.isSymbolicLink()) continue;
      const entryPath = path.join(directory, entry.name);
      if (entry.isDirectory()) walk(entryPath);
      else if (entry.isFile() && isMediaFile(entryPath)) paths.push(entryPath);
    }
  };
  walk(resolvedRoot);
  return paths.sort((a, b) => path.relative(resolvedRoot, a).localeCompare(path.relative(resolvedRoot, b)));
}

const PROFILE_SCHEMA_VERSION = 1;
const BUNDLED_HEADPHONE_FIR_PATTERN = /^headphone-compensation\/[a-z0-9][a-z0-9-]*\/[A-Za-z0-9][A-Za-z0-9._-]*\.f32$/;
const BUNDLED_HRTF_PATTERN = /^hrtf(?:-[a-z0-9]+)*\/(?:hrtf-set\.json|azm?\d+_elm?\d+_(?:dry|wet)\.f32)$/;
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

function isHeadTrackingHelperFile(helperPath) {
  try {
    return typeof helperPath === "string" &&
      path.extname(helperPath).toLowerCase() === ".exe" &&
      !fs.lstatSync(helperPath).isSymbolicLink() &&
      fs.statSync(helperPath).isFile();
  } catch {
    return false;
  }
}

function readHeadTrackingHelperPath() {
  const helperPath = readSettings().experimentalHeadTrackingHelperPath;
  return isHeadTrackingHelperFile(helperPath) ? helperPath : null;
}

function bundledHeadTrackingHelperPath() {
  if (process.platform !== "win32") return null;
  const candidates = app.isPackaged
    ? [path.join(process.resourcesPath, "head-tracking-helper", "SdaAirPodsHeadTracking.exe")]
    : [path.join(__dirname, "head-tracking-helper", "SdaAirPodsHeadTracking.exe")];
  return candidates.find(isHeadTrackingHelperFile) ?? null;
}

function resolvedHeadTrackingHelper() {
  const externalPath = readHeadTrackingHelperPath();
  if (externalPath) return { helperPath: externalPath, source: "external-helper" };
  const bundledPath = bundledHeadTrackingHelperPath();
  return bundledPath ? { helperPath: bundledPath, source: "bundled-helper" } : null;
}

function writeHeadTrackingHelperPath(helperPath) {
  if (helperPath !== null && !isHeadTrackingHelperFile(helperPath)) {
    throw new Error("head tracking helper must be a selected .exe regular file");
  }
  writeSettings({ experimentalHeadTrackingHelperPath: helperPath });
}

function readLastMediaDirectory() {
  const directory = readSettings().lastMediaDirectory;
  try {
    return typeof directory === "string" && fs.statSync(directory).isDirectory() ? directory : null;
  } catch {
    return null;
  }
}

// AirPods orientation is supplied by a separate GPL helper process. Keeping the
// device transport behind JSONL also prevents Bluetooth access from reaching the renderer.
const HEAD_TRACKING_PROTOCOL = 1;
const HEAD_TRACKING_MAX_LINE_BYTES = 4096;
const HEAD_TRACKING_MAX_BUFFER_BYTES = HEAD_TRACKING_MAX_LINE_BYTES * 2;
const HEAD_TRACKING_MAX_RATE_HZ = 120;
const HEAD_TRACKING_MAX_DIAGNOSTIC_CHARS = 240;
const HEAD_TRACKING_MOCK_INTERVAL_MS = 20;

// Native object renderer foundation. It is opt-in and starts in reference-mix
// mode only; Web Audio remains the audible fallback until object HRTF delivery.
const NATIVE_RENDERER_PROTOCOL = 1;
const NATIVE_RENDERER_MAX_LINE_BYTES = 16 * 1024;
let nativeRenderer = null;
let nativeRendererWritable = true;
let nativeRendererBuffer = "";
let nativeRendererStatus = { running: false, referenceMix: true, detail: "未启动" };

function bundledNativeRendererPath() {
  const executable = "SdaNativeRenderer.exe";
  const candidates = isDev
    ? [path.join(__dirname, "native-renderer", executable)]
    : [path.join(process.resourcesPath, "native-renderer", executable)];
  return candidates.find((candidate) => fs.existsSync(candidate)) ?? null;
}

function publishNativeRendererStatus() {
  for (const win of BrowserWindow.getAllWindows()) {
    if (!win.isDestroyed()) win.webContents.send("sda:native-renderer-status", nativeRendererStatus);
  }
  return nativeRendererStatus;
}

function setNativeRendererStatus(running, detail, referenceMix = true) {
  nativeRendererStatus = { running, referenceMix, detail: safeDiagnosticText(detail, "状态未知") };
  return publishNativeRendererStatus();
}

function nativeRendererCommand(command) {
  if (!nativeRenderer?.stdin || !nativeRendererWritable || !command || typeof command !== "object") return false;
  try {
    const json = Buffer.from(JSON.stringify(command), "utf8");
    if (json.length > NATIVE_RENDERER_MAX_LINE_BYTES) return false;
    const header = Buffer.allocUnsafe(5);
    header.writeUInt8("J".charCodeAt(0), 0);
    header.writeUInt32LE(json.length, 1);
    const accepted = nativeRenderer.stdin.write(Buffer.concat([header, json]));
    if (!accepted) nativeRendererWritable = false;
    return accepted;
  } catch {
    return false;
  }
}

function nativeRendererPcm(id, start, samples) {
  if (!nativeRenderer?.stdin || !nativeRendererWritable || typeof id !== "string" || !/^((obj:\d+)|(bed:\d+))$/.test(id)) return false;
  if (!Number.isSafeInteger(start) || start < 0 || !ArrayBuffer.isView(samples) || samples.BYTES_PER_ELEMENT !== 4) return false;
  const float = samples instanceof Float32Array
    ? samples
    : new Float32Array(samples.buffer, samples.byteOffset, Math.floor(samples.byteLength / 4));
  if (float.length === 0 || float.length > 480_000) return false;
  try {
    const idBytes = Buffer.from(id, "utf8");
    const header = Buffer.allocUnsafe(1 + 2 + 8 + 4);
    header.writeUInt8("P".charCodeAt(0), 0);
    header.writeUInt16LE(idBytes.length, 1);
    header.writeBigUInt64LE(BigInt(start), 3);
    header.writeUInt32LE(float.length, 11);
    const accepted = nativeRenderer.stdin.write(Buffer.concat([
      header,
      idBytes,
      Buffer.from(float.buffer, float.byteOffset, float.byteLength),
    ]));
    if (!accepted) nativeRendererWritable = false;
    return accepted;
  } catch {
    return false;
  }
}

function consumeNativeRendererOutput(chunk) {
  nativeRendererBuffer += chunk;
  if (nativeRendererBuffer.length > NATIVE_RENDERER_MAX_LINE_BYTES * 2) {
    nativeRendererBuffer = "";
    setNativeRendererStatus(false, "native renderer 输出超限，Web Audio 回退");
    return;
  }
  for (;;) {
    const newline = nativeRendererBuffer.indexOf("\n");
    if (newline < 0) break;
    const line = nativeRendererBuffer.slice(0, newline);
    nativeRendererBuffer = nativeRendererBuffer.slice(newline + 1);
    if (!line || line.length > NATIVE_RENDERER_MAX_LINE_BYTES) continue;
    try {
      const message = JSON.parse(line);
      if (message?.type === "ready" && message.protocol === NATIVE_RENDERER_PROTOCOL) {
        setNativeRendererStatus(true, `WASAPI ${message.sampleRate}Hz / ${message.outputChannels}ch（参考混音）`, true);
      } else if (message?.type === "error") {
        setNativeRendererStatus(false, `native renderer: ${safeDiagnosticText(message.detail, "错误")}`);
      } else if (message?.type === "health") {
        setNativeRendererStatus(true, `sample ${message.samplePos} · ${message.activeSources} source · ${message.underrunSamples} underrun`, Boolean(message.referenceMix));
      }
    } catch { /* malformed helper output is ignored; stderr still records it */ }
  }
}

function startNativeRenderer() {
  if (nativeRenderer) return nativeRendererStatus;
  const executable = bundledNativeRendererPath();
  if (!executable) return setNativeRendererStatus(false, "native renderer 未构建，Web Audio 回退");
  try {
    nativeRenderer = spawn(executable, [], { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
  } catch (error) {
    nativeRenderer = null;
    return setNativeRendererStatus(false, `native renderer 启动失败: ${error.message}`);
  }
  nativeRendererWritable = true;
  nativeRenderer.stdout.setEncoding("utf8");
  nativeRenderer.stdout.on("data", consumeNativeRendererOutput);
  nativeRenderer.stdin.on("drain", () => { nativeRendererWritable = true; });
  nativeRenderer.stderr.setEncoding("utf8");
  nativeRenderer.stderr.on("data", (chunk) => console.warn(`[SDA native renderer] ${String(chunk).trim()}`));
  nativeRenderer.once("error", (error) => {
    nativeRenderer = null;
    setNativeRendererStatus(false, `native renderer 异常: ${error.message}`);
  });
  nativeRenderer.once("exit", (code) => {
    nativeRenderer = null;
    setNativeRendererStatus(false, `native renderer 已退出${code === null ? "" : ` (${code})`}，Web Audio 回退`);
  });
  nativeRendererCommand({ type: "hello", protocol: NATIVE_RENDERER_PROTOCOL });
  return setNativeRendererStatus(false, "native renderer 启动中，Web Audio 回退");
}

function stopNativeRenderer() {
  const renderer = nativeRenderer;
  nativeRenderer = null;
  nativeRendererWritable = true;
  nativeRendererBuffer = "";
  if (renderer) {
    try { renderer.stdin?.write(`${JSON.stringify({ type: "shutdown" })}\n`); } catch {}
    setTimeout(() => { if (!renderer.killed) renderer.kill(); }, 500).unref();
  }
  return setNativeRendererStatus(false, "已停止，Web Audio 回退");
}

let headTrackingTimer = null;
let headTrackingStartedAt = 0;
let headTrackingHelper = null;
let headTrackingSession = null;
let headTrackingBuffer = "";
let headTrackingHelloReceived = false;
let headTrackingLastSequence = -1;
let headTrackingLastPoseAt = 0;
let headTrackingHelperSource = "bundled-helper";
let headTrackingEnabled = false;
let headTrackingStatus = { running: false, source: "bundled-helper", detail: "未启动" };

function sendHeadTracking(channel, payload) {
  for (const win of BrowserWindow.getAllWindows()) {
    if (!win.isDestroyed()) win.webContents.send(channel, payload);
  }
}

function publishHeadTrackingStatus() {
  sendHeadTracking("sda:head-tracking-status", headTrackingStatus);
  return headTrackingStatus;
}

function helperConfiguration() {
  const externalPath = readHeadTrackingHelperPath();
  const bundledPath = bundledHeadTrackingHelperPath();
  const helperPath = externalPath ?? bundledPath;
  return {
    configured: Boolean(helperPath),
    fileName: helperPath ? path.basename(helperPath) : null,
    bundledAvailable: Boolean(bundledPath),
    usingBundled: Boolean(bundledPath && !externalPath),
    externalSelected: Boolean(externalPath),
    mockAvailable: isDev && process.env.SDA_HEAD_TRACKING_MOCK === "1",
  };
}

function safeDiagnosticText(value, fallback) {
  if (typeof value !== "string") return fallback;
  const text = value.replace(/[\u0000-\u001f\u007f]/g, " ").trim();
  return text ? text.slice(0, HEAD_TRACKING_MAX_DIAGNOSTIC_CHARS) : fallback;
}

function setHeadTrackingStatus(running, source, detail) {
  headTrackingStatus = { running, source, detail: safeDiagnosticText(detail, "状态未知") };
  return publishHeadTrackingStatus();
}

function helperCommand(type) {
  if (!headTrackingHelper?.stdin || !headTrackingSession) return false;
  try {
    headTrackingHelper.stdin.write(`${JSON.stringify({ type, protocol: HEAD_TRACKING_PROTOCOL, session: headTrackingSession })}\n`);
    return true;
  } catch {
    return false;
  }
}

function validQuaternion(orientation) {
  if (!orientation || typeof orientation !== "object") return null;
  const values = [orientation.x, orientation.y, orientation.z, orientation.w];
  if (!values.every(Number.isFinite)) return null;
  const norm = Math.hypot(...values);
  if (norm < 0.99 || norm > 1.01) return null;
  return { x: values[0] / norm, y: values[1] / norm, z: values[2] / norm, w: values[3] / norm };
}

function processHeadTrackingMessage(message) {
  if (!message || typeof message !== "object" || Array.isArray(message)) {
    stopHeadTracking(true, "helper 消息无效");
    return;
  }
  const knownTypes = ["hello", "pose", "status", "error"];
  if (!knownTypes.includes(message.type)) return;
  if (message.protocol !== HEAD_TRACKING_PROTOCOL || message.session !== headTrackingSession) {
    stopHeadTracking(true, "helper 协议或会话无效");
    return;
  }
  if (!headTrackingHelloReceived) {
    if (
      message.type !== "hello" ||
      message.source !== "windows-airpods-experimental" ||
      message.coordinateSystem !== "sda-adm-right-forward-up" ||
      message.orientation !== "head-to-world-quaternion"
    ) {
      stopHeadTracking(true, "helper 握手无效");
      return;
    }
    headTrackingHelloReceived = true;
    setHeadTrackingStatus(true, headTrackingHelperSource, "已连接 helper，等待姿态");
    return;
  }
  if (message.type === "hello") {
    stopHeadTracking(true, "helper 重复握手");
    return;
  }
  if (message.type === "pose") {
    const orientation = validQuaternion(message.orientation);
    const sequence = message.seq;
    const timestampMs = message.timestampMs;
    const now = Date.now();
    if (
      !orientation ||
      !Number.isSafeInteger(sequence) || sequence <= headTrackingLastSequence ||
      !Number.isFinite(timestampMs) || Math.abs(timestampMs - now) > 60_000
    ) {
      stopHeadTracking(true, "helper 姿态无效");
      return;
    }
    if (now - headTrackingLastPoseAt < 1000 / HEAD_TRACKING_MAX_RATE_HZ) return;
    headTrackingLastSequence = sequence;
    headTrackingLastPoseAt = now;
    if (!headTrackingEnabled) return;
    setHeadTrackingStatus(true, headTrackingHelperSource, headTrackingHelperSource === "bundled-helper" ? "追踪中（内置 Windows helper）" : "追踪中（外部 helper）");
    sendHeadTracking("sda:head-tracking-pose", { timestampMs, orientation });
    return;
  }
  if (message.type === "status") {
    if (!["connected", "disconnected", "unavailable"].includes(message.state)) {
      stopHeadTracking(true, "helper 状态无效");
      return;
    }
    if (headTrackingEnabled) {
      setHeadTrackingStatus(true, headTrackingHelperSource, safeDiagnosticText(message.detail, message.state));
    }
    return;
  }
  const detail = safeDiagnosticText(message.message, safeDiagnosticText(message.code, "helper 错误"));
  setHeadTrackingStatus(true, headTrackingHelperSource, detail);
}

function consumeHeadTrackingOutput(chunk) {
  headTrackingBuffer += chunk;
  if (Buffer.byteLength(headTrackingBuffer, "utf8") > HEAD_TRACKING_MAX_BUFFER_BYTES) {
    stopHeadTracking(true, "helper 输出超出限制");
    return;
  }
  for (;;) {
    const newline = headTrackingBuffer.indexOf("\n");
    if (newline < 0) return;
    const line = headTrackingBuffer.slice(0, newline).replace(/\r$/, "");
    headTrackingBuffer = headTrackingBuffer.slice(newline + 1);
    if (!line) continue;
    if (Buffer.byteLength(line, "utf8") > HEAD_TRACKING_MAX_LINE_BYTES) {
      stopHeadTracking(true, "helper 消息过长");
      return;
    }
    try {
      processHeadTrackingMessage(JSON.parse(line));
    } catch {
      stopHeadTracking(true, "helper JSON 无效");
      return;
    }
  }
}

function mockHeadPose() {
  const elapsedSeconds = (Date.now() - headTrackingStartedAt) / 1000;
  const yawRadians = Math.sin(elapsedSeconds * 0.7) * (Math.PI / 6);
  return {
    timestampMs: Date.now(),
    orientation: { x: 0, y: 0, z: Math.sin(yawRadians / 2), w: Math.cos(yawRadians / 2) },
  };
}

function startHeadTrackingMock() {
  headTrackingStartedAt = Date.now();
  setHeadTrackingStatus(true, "mock", "模拟 yaw 姿态（仅开发验证）");
  headTrackingTimer = setInterval(() => sendHeadTracking("sda:head-tracking-pose", mockHeadPose()), HEAD_TRACKING_MOCK_INTERVAL_MS);
  headTrackingTimer.unref();
  return headTrackingStatus;
}

function startHeadTracking() {
  if (headTrackingTimer || headTrackingHelper) {
    headTrackingEnabled = true;
    writeHeadTrackingEnabled(true);
    const takeoverSent = takeoverHeadTracking();
    const hasRecentPose = Date.now() - headTrackingLastPoseAt < 1000;
    return setHeadTrackingStatus(
      true,
      headTrackingHelperSource,
      takeoverSent
        ? "Windows 正在强制接管整个 AirPods 连接"
        : hasRecentPose
        ? headTrackingHelperSource === "bundled-helper" ? "追踪中（内置 Windows helper）" : "追踪中（外部 helper）"
        : "正在恢复 AirPods motion",
    );
  }
  const resolvedHelper = resolvedHeadTrackingHelper();
  if (!resolvedHelper) {
    if (isDev && process.env.SDA_HEAD_TRACKING_MOCK === "1") return startHeadTrackingMock();
    throw new Error("内置 AirPods 头追 helper 不存在，请重新安装或选择外部 helper");
  }
  const { helperPath, source } = resolvedHelper;
  headTrackingHelperSource = source;
  headTrackingEnabled = true;
  writeHeadTrackingEnabled(true);
  headTrackingSession = crypto.randomBytes(32).toString("hex");
  headTrackingBuffer = "";
  headTrackingHelloReceived = false;
  headTrackingLastSequence = -1;
  headTrackingLastPoseAt = 0;
  try {
    headTrackingHelper = spawn(helperPath, [], { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
  } catch (error) {
    headTrackingHelper = null;
    headTrackingSession = null;
    writeHeadTrackingEnabled(false);
    throw error;
  }
  headTrackingHelper.stdout.setEncoding("utf8");
  headTrackingHelper.stdout.on("data", consumeHeadTrackingOutput);
  headTrackingHelper.stderr.setEncoding("utf8");
  headTrackingHelper.stderr.on("data", (chunk) => {
    // Helper diagnostics are env-gated (SDA_HEAD_TRACKING_DEBUG=1); forward
    // them to the Electron console in dev so reproduction logs are readable.
    if (!isDev) return;
    for (const line of chunk.split(/\r?\n/)) {
      if (line) console.log("[SDA helper]", line);
    }
  });
  headTrackingHelper.stdin.once("error", () => {
    if (headTrackingHelper) stopHeadTracking(true, "helper 命令通道失败");
  });
  headTrackingHelper.once("error", (error) => stopHeadTracking(true, `helper 启动失败: ${error.message}`));
  headTrackingHelper.once("exit", (code) => {
    if (headTrackingHelper) stopHeadTracking(true, `helper 已退出${code === null ? "" : ` (${code})`}`);
  });
  helperCommand("start");
  const takeoverSent = takeoverHeadTracking();
  return setHeadTrackingStatus(
    true,
    headTrackingHelperSource,
    takeoverSent ? "Windows 正在强制接管整个 AirPods 连接" : "正在连接 AirPods motion 通道",
  );
}

function stopHeadTracking(persist = true, detail = "已停止") {
  if (headTrackingTimer) clearInterval(headTrackingTimer);
  headTrackingTimer = null;
  const helper = headTrackingHelper;
  if (helper) {
    helperCommand("stop");
    headTrackingHelper = null;
    helper.removeAllListeners();
    helper.stdout?.removeAllListeners();
    helper.stderr?.removeAllListeners();
    try { helper.kill(); } catch {}
  }
  headTrackingSession = null;
  headTrackingBuffer = "";
  headTrackingHelloReceived = false;
  headTrackingEnabled = false;
  if (persist) writeHeadTrackingEnabled(false);
  return setHeadTrackingStatus(false, headTrackingHelperSource, detail);
}

async function stopHeadTrackingGracefully(persist = true, detail = "已停止") {
  if (headTrackingTimer) return stopHeadTracking(persist, detail);
  const helper = headTrackingHelper;
  if (!helper) return stopHeadTracking(persist, detail);

  // Let the helper send both AirPods stop packets and close L2CAP before a new
  // helper is allowed to connect. Killing it immediately leaves the buds in a
  // stale motion session and the next start waits forever for motion packets.
  helperCommand("stop");
  headTrackingHelper = null;
  headTrackingEnabled = false;
  headTrackingSession = null;
  headTrackingBuffer = "";
  headTrackingHelloReceived = false;
  helper.stdout?.removeAllListeners("data");
  helper.stderr?.removeAllListeners("data");
  helper.stdin?.removeAllListeners("error");
  helper.removeAllListeners("error");
  helper.removeAllListeners("exit");

  await new Promise((resolve) => {
    let finished = false;
    let killTimer;
    let forceTimer;
    const finish = () => {
      if (finished) return;
      finished = true;
      clearTimeout(killTimer);
      clearTimeout(forceTimer);
      resolve();
    };
    helper.once("exit", finish);
    helper.once("error", finish);
    killTimer = setTimeout(() => {
      try { helper.kill(); } catch { finish(); }
    }, 750);
    forceTimer = setTimeout(finish, 1500);
    killTimer.unref();
    forceTimer.unref();
  });

  if (persist) writeHeadTrackingEnabled(false);
  return setHeadTrackingStatus(false, headTrackingHelperSource, detail);
}

function suspendHeadTracking() {
  if (headTrackingTimer || !headTrackingHelper) return stopHeadTracking(true, "已停止");
  headTrackingEnabled = false;
  writeHeadTrackingEnabled(false);
  return setHeadTrackingStatus(false, headTrackingHelperSource, "已关闭（保持 AirPods motion 连接）");
}

function recenterHeadTracking() {
  if (!headTrackingTimer && !headTrackingHelper) throw new Error("head tracking is not running");
  if (headTrackingTimer) {
    headTrackingStartedAt = Date.now();
    const pose = mockHeadPose();
    sendHeadTracking("sda:head-tracking-recenter", pose);
    return pose;
  }
  helperCommand("recenter");
  // Renderer-side recenter remains available even for helpers without a command API.
  sendHeadTracking("sda:head-tracking-recenter", null);
  return null;
}

function takeoverHeadTracking() {
  if (
    !headTrackingEnabled ||
    !headTrackingHelper ||
    headTrackingHelperSource !== "bundled-helper"
  ) return false;
  const sent = helperCommand("takeover");
  if (sent) setHeadTrackingStatus(true, headTrackingHelperSource, "Windows 正在强制接管整个 AirPods 连接");
  return sent;
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
ipcMain.handle("sda:head-tracking-helper", () => helperConfiguration());
ipcMain.handle("sda:head-tracking-select-helper", async (event) => {
  if (headTrackingStatus.running) throw new Error("stop head tracking before selecting a helper");
  if (headTrackingTimer || headTrackingHelper) await stopHeadTrackingGracefully(false);
  const parent = BrowserWindow.fromWebContents(event.sender);
  const options = {
    title: "选择独立 AirPods 头追 helper",
    filters: [{ name: "Windows helper", extensions: ["exe"] }],
    properties: ["openFile"],
  };
  const { canceled, filePaths } = parent
    ? await dialog.showOpenDialog(parent, options)
    : await dialog.showOpenDialog(options);
  if (canceled) return helperConfiguration();
  const helperPath = path.resolve(filePaths[0]);
  writeHeadTrackingHelperPath(helperPath);
  if (!headTrackingStatus.running) setHeadTrackingStatus(false, "external-helper", `已配置 ${path.basename(helperPath)}`);
  return helperConfiguration();
});
ipcMain.handle("sda:head-tracking-use-bundled-helper", async () => {
  if (headTrackingStatus.running) throw new Error("stop head tracking before changing helper");
  if (headTrackingTimer || headTrackingHelper) await stopHeadTrackingGracefully(false);
  writeHeadTrackingHelperPath(null);
  const configuration = helperConfiguration();
  if (!configuration.bundledAvailable) throw new Error("bundled head tracking helper is unavailable");
  headTrackingHelperSource = "bundled-helper";
  setHeadTrackingStatus(false, "bundled-helper", "已选择内置 Windows helper");
  return configuration;
});
ipcMain.handle("sda:head-tracking-start", () => startHeadTracking());
ipcMain.handle("sda:head-tracking-stop", () => suspendHeadTracking());
ipcMain.handle("sda:native-renderer-status", () => nativeRendererStatus);
ipcMain.handle("sda:native-renderer-start", () => startNativeRenderer());
ipcMain.handle("sda:native-renderer-stop", () => stopNativeRenderer());
ipcMain.handle("sda:native-renderer-health", () => {
  nativeRendererCommand({ type: "health" });
  return nativeRendererStatus;
});
ipcMain.handle("sda:native-renderer-source", (_event, id, atSample) => {
  if (!/^((obj:\d+)|(bed:\d+))$/.test(id ?? "") || !Number.isSafeInteger(atSample) || atSample < 0) return false;
  return nativeRendererCommand({ type: "addSource", id });
});
ipcMain.handle("sda:native-renderer-frame", (_event, samplePos, entries) => {
  if (!Number.isSafeInteger(samplePos) || samplePos < 0 || !Array.isArray(entries) || entries.length > 64) return false;
  return entries.every((entry) => nativeRendererPcm(entry?.id, samplePos, entry?.samples));
});
ipcMain.handle("sda:head-tracking-recenter", () => recenterHeadTracking());

ipcMain.handle("sda:pick-file", async (event) => {
  // 挂到发起窗口上：弹窗跟随主窗口置顶，不会跑到后台/其他显示器；
  // 且异步版本不会冻结主进程事件循环，播放中的 IPC 读文件不受影响。
  const parent = BrowserWindow.fromWebContents(event.sender);
  const defaultPath = readLastMediaDirectory();
  const options = {
    ...(defaultPath ? { defaultPath } : {}),
    filters: [
      { name: "Spatial audio", extensions: MEDIA_DIALOG_EXTENSIONS },
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

ipcMain.handle("sda:pick-folder", async (event) => {
  const parent = BrowserWindow.fromWebContents(event.sender);
  const defaultPath = readLastMediaDirectory();
  const options = { ...(defaultPath ? { defaultPath } : {}), properties: ["openDirectory"] };
  const { canceled, filePaths } = parent
    ? await dialog.showOpenDialog(parent, options)
    : await dialog.showOpenDialog(options);
  const folderPath = canceled ? null : filePaths[0] ?? null;
  if (!folderPath) return { canceled: true, paths: [] };
  try {
    writeSettings({ lastMediaDirectory: folderPath });
  } catch (error) {
    console.warn("[SDA] 最近媒体目录未保存:", error);
  }
  return { canceled: false, paths: scanMediaFolder(folderPath) };
});

ipcMain.handle("sda:open-path", (_e, filePath) => {
  if (typeof filePath !== "string" || !isMediaFile(filePath)) throw new Error("unsupported media path");
  const stat = fs.statSync(filePath);
  if (!stat.isFile()) throw new Error("media path is not a regular file");
  const id = nextFileId++;
  openFiles.set(id, filePath);
  return { id, size: stat.size, name: path.basename(filePath) };
});

ipcMain.handle("sda:read-slice", (_e, id, offset, length) => {
  const filePath = openFiles.get(id);
  if (!filePath) throw new Error("unknown file id");
  if (!Number.isSafeInteger(offset) || offset < 0 || !Number.isSafeInteger(length) || length < 0 || length > MAX_SLICE_BYTES) {
    throw new Error("invalid file slice");
  }
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
  const setDir = assetPath.split("/")[0];
  const hrtfRoot = path.resolve(root, setDir);
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
  if (readHeadTrackingEnabled() && resolvedHeadTrackingHelper()) {
    try { startHeadTracking(); } catch (error) { console.warn("[SDA] 头部追踪自动启动失败:", error); }
  }
  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", async () => {
  await stopHeadTrackingGracefully(false);
  stopNativeRenderer();
  if (process.platform !== "darwin") app.quit();
});
