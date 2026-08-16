# SDA — Spatial Decoder App

把 [harletty-bridge](https://github.com/harletty/harletty-bridge) 的
Dolby TrueHD / E-AC-3(Atmos JOC) / DTS 解码器带到网页、桌面和移动端：
解码出**每对象 PCM + 3D 空间元数据**，用 VBAP + HRTF 双耳渲染到耳机，
或多声道渲染到音箱，并实时可视化每个声音对象的运动（对标
Omniphony Studio 的 3D 视图）。

```
文件 (.mkv/.mp4/.thd/.ec3/.dts)
   │  @sda/demux    解封装 → 访问单元 (AU)
   ▼
@sda/core (Rust→WASM, harletty 解码 crate)
   │  解码 → 平面 PCM (bed + 每对象一路) + 对象事件 {位置,增益,size,ramp}
   ▼
@sda/renderer (Web Audio AudioWorklet)
   │  VBAP 3D → N 条虚拟扬声器总线 →
   │    双耳: BRIR 卷积 (KEMAR) / PannerNode HRTF 降级
   │    多声道: 直通音箱 | 立体声: 下混
   ▼
@sda/player  调度（worker 解码、ring buffer、元数据时间轴）
   │
   ▼  React UI + three.js 3D 对象视图
```

## 目录

| 路径 | 说明 |
|---|---|
| `packages/core` | Rust→WASM 解码核心（truehd/eac3/dca crate 来自 harletty-bridge） |
| `packages/demux` | MKV (EBML) / MP4 流式解封装 |
| `packages/renderer` | VBAP 3D panning + AudioWorklet 混音 + 双耳/多声道输出 |
| `packages/player` | 解码 worker + 播放调度 + 可视化状态 |
| `apps/web` | 网页版（Vite + React + TS + three.js） |
| `apps/desktop` | 桌面版（Electron，复用 web 构建产物） |
| `apps/head-tracking-helper` | Windows AirPods AACP 头部追踪 helper（独立 GPL-3.0-or-later 进程） |
| `apps/head-tracking-driver` | Windows KMDF Bluetooth L2CAP profile 驱动、INF 与测试签名包 |
| `apps/mobile` | 手机版（Expo；解码走原生模块，见 docs） |
| `harletty-bridge` | 上游解码器仓库（clone/submodule） |
| `Omniphony` | 上游渲染器仓库（参考实现，含 BINAURAL.md） |
| `docs/binaural-rendering.md` | 双耳渲染设计（Dolby/Apple 官方调研） |
| `docs/mobile-native-module.md` | Expo 原生模块设计 |

## 新电脑从零开始（完整搭建指南）

本项目**没有后端**，所有解码在浏览器/本机完成（Rust 解码器编译成 WASM）。
只需一次性装好工具链即可正常开发。

### 1. 安装工具链

| 工具 | 版本要求 | 安装 |
|---|---|---|
| Node.js | ≥ 20（开发用 24） | https://nodejs.org |
| pnpm | ≥ 9 | `npm install -g pnpm`（或 `corepack enable && corepack prepare pnpm@latest --activate`） |
| Rust | 稳定版（开发用 1.96） | https://rustup.rs 一键安装 |
| wasm target | — | `rustup target add wasm32-unknown-unknown` |
| wasm-bindgen-cli | **必须 = 0.2.127**（与 `packages/core/Cargo.lock` 一致） | `cargo install wasm-bindgen-cli --version 0.2.127` |

> wasm-bindgen-cli 版本和 Cargo.lock 里的 wasm-bindgen crate 不一致会报错；
> 若升级了 crate，重新 `cargo install wasm-bindgen-cli --version <新版本>`。

### 2. 拿代码

本项目是 git 仓库（https://github.com/fengluoxiao/SDA），`harletty-bridge` 以
**git submodule** 形式挂在仓库里（fork 自上游，锁定在 `fcf1c00`，
地址 https://github.com/fengluoxiao/harletty-bridge），克隆时一条命令一起拉下来：

```bash
# 克隆主项目 + 自动克隆 harletty-bridge（必须带 --recurse-submodules）
git clone --recurse-submodules https://github.com/fengluoxiao/SDA.git SDA

# 如果已经克隆了但没带子模块，补拉：
git submodule update --init --recursive
```

`Omniphony`（~24 MB）是上游渲染器参考实现，只在文档/注释里提及，
构建和运行都不依赖，**没有纳入仓库**，不需要拷。

> WASM 产物（`packages/core/pkg-web`、`pkg-node`）不入库，克隆后需要
> 按第 1 步装好 Rust 工具链，再执行第 4 步 `pnpm core:build` 生成。

### 3. 安装依赖

```bash
pnpm install
```

### 4. 构建 WASM 解码核心（仅当没有 pkg-web/pkg-node 时）

```bash
pnpm core:build        # cargo build --target wasm32 + wasm-bindgen → pkg-web(浏览器) / pkg-node(测试)
```

可选验证：`cd packages/core && pnpm test`（用 harletty-bridge 仓库里的 JOC 测试向量冒烟）。

### 4.5 生成双耳 HRTF 资产（仅当没有 apps/web/public/hrtf 时）

双耳模式用的头相关脉冲响应来自 **SADIE II 数据库**（University of York，
**Apache-2.0**，D1 = Neumann KU100 人工头，含消声室 HRIR + 房间 BRIR）。
从 [Zenodo](https://doi.org/10.5281/zenodo.12092466) 下载 `D1.zip`（~117MB，
48kHz WAV），然后：

```bash
# D1.zip 同时含 HRIR/BRIR，用路径过滤各取所需；也可以直接传 zip 的 http(s) 地址
pnpm hrtf:build -- --hr D1.zip --br D1.zip \
  --hr-path D1_HRIR_WAV/48K_24bit --br-path D1_BRIR_WAV/48K_24bit
# → apps/web/public/hrtf/hrtf-set.json + 每个方向的 dry/wet .f32
```

没有这些资产时双耳模式仍能工作（回退浏览器内置 HRTF），但方位精度和
房间感会大打折扣。设计依据见
`docs/binaural-rendering.md`（杜比 BS.2127 虚拟音箱+BRIR 管线 /
苹果 inverse 距离定律）。

### 5. 日常使用

```bash
# 网页版开发（热更新）
pnpm web:dev                 # http://localhost:5173

# 网页版生产构建 + 本地预览
pnpm web:build               # 产物在 apps/web/dist
cd apps/web && npx vite preview   # http://localhost:4173

# 桌面版（Electron，复用 web 产物）
pnpm web:build && pnpm desktop:dev

# 手机版（Expo 壳）
pnpm mobile:start
```

### 6. 打包桌面安装包（Windows）

```bash
cd apps/desktop

# 国内网络先设 Electron 下载镜像（否则 GitHub 下不动）
export ELECTRON_MIRROR="https://npmmirror.com/mirrors/electron/"

# 构建 helper、校验/暂存驱动、构建 web 并打包
pnpm --filter @sda/desktop build -- --win nsis
```

产物在 `apps/desktop/dist/`：

- `SDA Setup x.y.z.exe` —— NSIS 安装包（推荐分发）
- `win-unpacked/` —— 免安装绿色版，直接运行里面的 `SDA.exe`

Windows assisted installer 提供两个默认不勾选的 AirPods 头追选项：开启
TestSigning 和安装测试签名驱动。只有用户勾选后才会请求管理员权限；安装器不会
自动重启。完整安全说明、日志位置和恢复步骤见
[`docs/windows-head-tracking-install.md`](docs/windows-head-tracking-install.md)。

**注意：不要设 `ELECTRON_BUILDER_BINARIES_MIRROR`**。electron-builder 每次运行都会
重新下载 winCodeSign 工具包，其中包含 macOS 的 `.dylib` 符号链接，Windows 无
开发者模式/管理员权限时 7zip 解压必然报「客户端没有所需的特权」。已在
`apps/desktop/package.json` 里设了 `win.signAndEditExecutable: false` 跳过该步骤
（代价：exe 用 Electron 默认图标、无数字签名）。要加图标/签名时，需开启
Windows「开发者模式」后去掉该选项再配 `win.icon` / 证书。

## Windows AirPods 头部追踪（0.2.x）

Windows 版通过一个独立 Rust helper 和一个实验性 KMDF Bluetooth profile 驱动，
从 AirPods 的 AACP L2CAP 通道读取 motion 姿态。Electron 只负责 helper 生命周期和
JSON Lines IPC；渲染器接收设备无关的 `head-to-world-quaternion` 四元数。协议定义见
[`docs/head-tracking-helper-jsonl-protocol.md`](docs/head-tracking-helper-jsonl-protocol.md)。

### LibrePods 来源与许可

AirPods AACP 握手、motion stream 请求包、Smart Routing 接管、接近配对型号/PID
映射以及实测姿态轴映射，基于
[LibrePods](https://github.com/librepods-org/librepods) 的研究成果，
Copyright (C) 2025 LibrePods contributors。

- LibrePods 使用 `GPL-3.0-or-later`；`apps/head-tracking-helper` 同样作为
  `GPL-3.0-or-later` 的独立可执行程序分发，并随包附带完整许可证。
- SDA 主程序不链接 helper；两者只通过已文档化的 stdin/stdout JSONL 协议通信。
- `apps/head-tracking-driver` 基于 Microsoft Bluetooth Echo L2CAP Profile Driver
  sample（commit `717778a20ba4dd2440fe609f69153a1f8a64f597`），保留 Microsoft
  Public License 和原始版权声明；其 AirPods 硬件白名单来自 LibrePods 型号映射。
- 重新分发或拆出独立项目时，必须保留 LibrePods 的归属、helper 的 GPL 源码与许可，
  以及驱动中的 [`LICENSE.microsoft.txt`](apps/head-tracking-driver/LICENSE.microsoft.txt)。

本功能不是 Apple 官方实现。AirPods、Dolby 等商标归各自权利人所有。

### 支持范围与运行要求

已在 Windows 11 x64 上验证。AirPods 必须先在 Windows 设置中完成配对并连接媒体音频。
驱动只匹配 Apple Bluetooth VID、AACP UUID 和以下明确 PID 的组合，不会接管普通
Bluetooth、A2DP、HFP 或 AVRCP 驱动：

| 型号 | PID |
|---|---|
| AirPods Pro | `200E` |
| AirPods Pro 2（Lightning / USB-C） | `2014` / `2024` |
| AirPods 3 | `2013` |
| AirPods 4（标准 / ANC） | `2019` / `201B` |
| AirPods Max（Lightning / USB-C） | `200A` / `201F` |

AirPods 1/2 没有受支持的 motion 数据，未知型号也不会猜测匹配。使用当前测试签名驱动
需要管理员权限、将测试证书加入本机 `Root` 与 `TrustedPublisher`、开启 Windows
TestSigning 并重启。Secure Boot 策略可能阻止开启 TestSigning；不要仅为头追盲目
关闭 Secure Boot。安装、恢复与日志位置见
[`docs/windows-head-tracking-install.md`](docs/windows-head-tracking-install.md)。

### 构建依赖

| 部分 | 依赖 |
|---|---|
| 整体工作区 | Git（克隆时带 submodule）、Node.js >= 20（CI 使用 24）、pnpm 9 |
| Rust helper | stable Rust；推荐 `x86_64-pc-windows-msvc`，GNU 工具链需 MSYS2 MinGW64 的 `as.exe` / `dlltool.exe` |
| 解码核心 | `wasm32-unknown-unknown`、与锁文件一致的 `wasm-bindgen-cli 0.2.127` |
| KMDF 驱动 | Visual Studio 2022 或 Build Tools、MSVC x64 C++ 工具、Windows SDK + WDK `10.0.26100.0` |
| 安装包 | PowerShell 5.1、Electron/electron-builder；NSIS 由 electron-builder 获取 |

只修改 helper 时不需要安装 WDK；使用仓库中已签名的
`apps/head-tracking-driver/package` 打包时也不需要重新编译驱动。

### 构建与测试 helper

在仓库根目录运行：

```powershell
pnpm install --frozen-lockfile
pnpm head-tracking:test
pnpm head-tracking:test-js
pnpm head-tracking:build
```

最后一条命令生成 release helper，并暂存到
`apps/desktop/head-tracking-helper/SdaAirPodsHeadTracking.exe`。若使用 GNU Rust，
构建脚本会自动查找 Rust 自带工具和 `C:\msys64\mingw64\bin`。

### 从源码构建并测试签名驱动

仓库已经提交可安装的 `.inf/.sys/.cat/.cer` 测试签名包，普通应用打包直接使用即可。
只有修改 `apps/head-tracking-driver/src`、INF 或硬件白名单时才需要执行本节。建议在
“Developer PowerShell for VS 2022”中运行，并在构建前更新
`SdaAirPodsL2cap.inx` 的 `DriverVer`：

```powershell
msbuild apps\head-tracking-driver\SdaAirPodsL2cap.vcxproj `
  /p:Configuration=Release /p:Platform=x64
```

主要输出位于 `apps\head-tracking-driver\build\x64\Release`。项目设置为
`DriverSign=Off`，仓库不保存任何私钥；自行构建者必须创建自己的代码签名证书，
先签 `.sys`，再重新生成并签 `.cat`。下面命令需在包含 WDK `Inf2Cat` 和
`SignTool` 的 VS 开发终端中执行：

```powershell
$DriverRoot = Resolve-Path 'apps\head-tracking-driver'
$BuildRoot = Join-Path $DriverRoot 'build\x64\Release'
$Package = Join-Path $env:TEMP 'SdaAirPodsDriverPackage'
New-Item -ItemType Directory -Path $Package -Force | Out-Null

Copy-Item (Join-Path $BuildRoot 'SdaAirPodsL2cap.inf') $Package -Force
Copy-Item (Join-Path $BuildRoot 'SdaAirPodsL2cap.sys') $Package -Force

$Cert = New-SelfSignedCertificate `
  -Type CodeSigningCert `
  -Subject 'CN=SDA AirPods Test Driver' `
  -CertStoreLocation 'Cert:\CurrentUser\My' `
  -KeyExportPolicy Exportable `
  -HashAlgorithm SHA256 `
  -NotAfter (Get-Date).AddYears(10)

SignTool sign /v /fd SHA256 /sha1 $Cert.Thumbprint `
  (Join-Path $Package 'SdaAirPodsL2cap.sys')
Inf2Cat /driver:$Package /os:10_X64 /uselocaltime
SignTool sign /v /fd SHA256 /sha1 $Cert.Thumbprint `
  (Join-Path $Package 'SdaAirPodsL2cap.cat')
Export-Certificate -Cert $Cert `
  -FilePath (Join-Path $Package 'SdaAirPodsL2cap.cer') | Out-Null
```

不要用仓库中的 `.cer` 给新驱动签名：它只有公钥，没有私钥。发布自行构建的驱动时，
把四个新文件放入 `apps/head-tracking-driver/package/`，然后同步更新以下三处，否则
安装包会因完整性校验失败而停止：

1. `scripts/stage-head-tracking-driver.mjs` 中证书及四个包文件的 SHA-256。
2. `apps/desktop/test/head-tracking-installer-contract.test.mjs` 中相同的 SHA-256。
3. `apps/desktop/installer/install-head-tracking-driver.ps1` 中证书 SHA-256。

可用 `Get-FileHash -Algorithm SHA256 <文件>` 取得新哈希。驱动包校验和安装器契约测试：

```powershell
node scripts/stage-head-tracking-driver.mjs
pnpm head-tracking:test-js
```

### 打包完整 Windows 预览版

```powershell
$env:ELECTRON_MIRROR = 'https://npmmirror.com/mirrors/electron/' # 国内网络可选
pnpm --filter @sda/desktop build -- --win nsis --x64 --publish never
```

该命令依次构建 helper、校验并暂存驱动、构建 web，然后生成
`apps/desktop/dist/SDA Setup <version>-x64.exe`。安装向导中的 TestSigning 和驱动安装
复选框默认都不勾选，只有用户选择后才会提权执行 PowerShell，也不会自动重启。

`.github/workflows/windows-preview.yml` 在推送 `feat/airpods-head-tracking` 时执行同一套
测试和打包，并上传保留 14 天的 Windows x64 artifact。

用法：拖入含 TrueHD/Atmos 或 E-AC-3 JOC 音轨的 `.mkv`（或裸 `.thd`/`.ec3`/`.dts`），
主视图是实时 3D 对象位置，底栏是迷你播放器（暂停/重播/音量），
顶栏可切换输出模式（双耳/立体声/多声道）、音箱布局（默认"自动"，按码流
内容检测；可手动 5.1 ~ 9.1.6）和深浅色主题。双耳距离固定"近"（杜比
Binaural Settings 语义，引擎保留近/中/远机制）。

### MDR-7506 平均测量 EQ

双耳输出可选内置 `Sony MDR-7506（AutoEq 平均测量 EQ，L/R 同一曲线）`。
它从公开的 AutoEq Super Review 单一/平均响应派生，并在最终双耳合并后为左右
输出分别运行同一份不可变 FIR；没有 crossfeed，也不会改变虚拟扬声器或 KU100
BRIR/HRTF 资产。

该档案不是独立左右声道测量，也不校正特定耳机的 driver 偏差、耳罩磨损或佩戴
密封差异。FIR 保持 1 kHz 归一化，runtime profile branch 施加由发布 FIR 峰值
校核的 `-6.1 dB` preamp；它不添加响度控制或压缩。既有 binaural makeup 与
stereo-linked lookahead limiter 保持独立行为。可审计的来源、生成方法、SHA-256 与限制见
`apps/web/public/headphone-compensation/sony-mdr-7506-average-autoeq/`。

### 常见问题

- **端口被占用**：5173 是 dev server（热更新），4173 是 preview（需先 build）。
  两者可以同时存在，注意别看错标签页。
- **改了 Rust 解码代码**：重新 `pnpm core:build` 再刷新页面。
- **改了 TS/React 代码**：dev server 自动热更新；preview 需要重新 `pnpm web:build`。
- **worklet 改了不生效**：`packages/renderer/worklet/*.js` 是静态资源，
  build 后浏览器可能缓存，强制刷新（Ctrl+F5）。

## 构建与运行（简版）

前置：Rust (+ `wasm32-unknown-unknown` target)、wasm-bindgen-cli、Node ≥ 20、pnpm。

```bash
# 1. 构建 wasm 解码核心（pkg-web + pkg-node）
pnpm core:build          # = node scripts/build-core.mjs

# 2. 验证解码（Node 冒烟测试，用 harletty 仓库的 JOC 测试向量）
cd packages/core && pnpm test

# 3. 安装依赖 & 启动网页版
pnpm install
pnpm web:dev             # http://localhost:5173

# 4. 桌面版
pnpm --filter @sda/web build
pnpm desktop:dev         # 或 SDA_DEV_URL 指向 vite dev server

# 5. 手机版（UI 壳；原生解码模块见 docs/mobile-native-module.md）
pnpm mobile:start
```

用法：拖入含 TrueHD/Atmos 或 E-AC-3 JOC 音轨的 `.mkv`（或裸 `.thd`/`.ec3`），
右侧看到对象列表和码流信息，主视图是实时 3D 对象位置。
输出模式：双耳（耳机）/ 立体声 / 多声道。

## 关键技术决策

- **解码器不重写**：harletty 的三个纯 Rust 解码 crate 直接编到 wasm
  （已验证：JOC 测试向量解出 5.1 bed + 15 对象 + 15 条空间事件）；
  移动端同一份 Rust 编静态库。
- **bridge ABI 不搬**：`bridge/` 是 Omniphony 私有 ABI 的胶水，网页版用
  干净的 `push(bytes) → nextFrame()` 接口重写（事件模型对齐
  `bridge_api::REvent`，坐标为 ADM 笛卡尔）。
- **渲染器自研**：Omniphony 的 VBAP 渲染不在 harletty 仓库，按其
  `BINAURAL.md` 与 EBU BEAR 的架构（虚拟扬声器 + BRIR 卷积）重写。
- **坐标系**：事件流是 ADM 约定（x+右, y+前, z+上），Web Audio/three.js
  是右手系（x+右, y+上, z+朝听者），转换集中在 `renderer/src/coords.ts`。

## 许可

除下列明确标注的组件外，SDA 与解码 crate 使用 Apache-2.0
（包括 truehdd 项目及 harletty-bridge）：

- `apps/head-tracking-helper`：GPL-3.0-or-later，协议研究与姿态映射归属 LibrePods。
- `apps/head-tracking-driver`：基于 Microsoft Bluetooth Echo L2CAP sample，适用
  Microsoft Public License；详见组件内许可证与版权声明。
- HRTF/BRIR 与耳机校正资产按各自目录中记录的来源和许可分发。

Dolby/DTS/Apple/AirPods 商标与相关专利归各自所有者；本项目是解码、渲染与设备
互操作技术研究，不提供任何受版权保护的测试内容。MIT KEMAR HRTF 数据集声明为
"free with no restrictions on use"（使用时仍应引用）。
