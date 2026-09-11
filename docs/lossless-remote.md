# 双设备无损远程播放

## 使用

主机安装 SDA，接收端使用现代浏览器即可，不需要安装 SDA、Python 或额外服务。当前双设备授权流程面向网页客户端；旧版 SDA 原生客户端仍可连接未启用设备授权的旧主机，新主机不再接受共享配对密钥直接用于原生收听。

1. 主机打开「系统设置 → 无损远程」，选择端口和缓冲，点击「开启发送」。
2. 主机点击对应网卡的「复制网页链接」。在局域网选择局域网 IP，异地组网选择 Tailscale / ZeroTier 网卡 IP。
3. iPhone、iPad 或电脑浏览器打开 HTTPS 链接，确认主机证书，填写设备名称后点击「连接并收听」。首次连接需在电脑的远程设置中批准，可选「仅收听」或「允许收听与控制」。已授权设备以后可留空密钥直接重连。
4. 主机添加歌曲；远端可选歌曲、播放、暂停、上一首、下一首、从头播放、调主机音量、切换循环模式和立体声渲染方式。
5. 「断开连接」结束客户端；「结束发送」关闭主机服务并停止接受连接。本机播放独立保留，手机断开或结束发送不会暂停本机。

默认 TCP 端口 49632，可在开启前修改。Windows 防火墙需允许该端口的入站连接；组网工具须提供两机之间的双向 TCP 连通。使用已有异地虚拟组网时无需公网端口映射，也不自动安装组网工具或更改防火墙。配对地址支持手动把主机部分换为可达的 DNS 名称或 IPv4 地址，其余部分保持完整。

同一主机最多两台已授权设备同时收听，第三台被拒绝，不挤掉已连接设备；持有原会话 Cookie 的同一浏览器可重新建立收听会话，旧会话随之失效。发送端和接收端互斥，不能同时担任两个角色。功能默认关闭，重启不会自行开启监听或连接上一次的主机。

## 音频边界

原生客户端及网页低延迟 PCM 模式的传输内容为 SDA 原生渲染器最后一级处理后的 **48,000 Hz、双声道、IEEE 754 float32 小端交错 PCM**，不是未渲染的 ADM 对象，也不是系统声音采集。PCM 有效载荷约 **3.072 Mbps**，网络与 TLS 另有少量开销。

主机的房间、HRTF、耳机模拟、Master 增益等处理已包含在结果中。客户端不再执行这些处理、不加淡入、不做响度归一化。网页使用 48 kHz AudioWorklet 直接输出收到的浮点样本，原生接收器直接进入 WASAPI。OS 静音或主机输出设备缺失不影响内部取流；SDA 自己的 Master 音量仍属于渲染结果，远端音量控件明确标为「主机音量」。

“无损”保证渲染结果经过网络到接收 FIFO 的浮点样本不被有损编码或改变。物理输出是另一层：

- 网页从 WSS 接收原始 PCM，AudioWorklet FIFO 不改变样本；浏览器和操作系统最终仍可能混音、调音量或重采样，不承诺物理设备位完美。浏览器须支持安全上下文、WebSocket 和 AudioWorklet，并允许播放声音。
- 原生独占输出仅协商 48 kHz，优先 float32，再尝试 PCM32 / PCM24 / PCM16；界面显示实际设备格式。整数 DAC 的格式转换存在量化，不能把 float32 网络传输等同于所有 DAC 都支持 float32 位完美。
- 共享输出要求系统设备格式为 48 kHz，不支持时明确报错；系统混音、APO、系统音量仍可能影响最终输出。
- 没有可用设备、独占被占用、设备断开时停止收听并显示错误，不回退有损编码、不暗中选择其他扬声器。

## 缓冲、时钟与断线

可以选 100 / 300 / 600 / 1000 ms 网络窗口。局域网通常选择 100 或 300 ms，异地组网可选择 600 ms。窗口为在途音频上限，实际端到端延迟还包含主机渲染预取、网络往返及设备缓冲。

远端 AudioWorklet / WASAPI 消费速度回传累计释放的帧数，发送端据此发放有限音频额度。本机声卡可用时，本机时钟推动渲染，远端使用独立的 6 秒 PCM FIFO，并按接收额度传输，不与声卡争抢样本。远端持续落后超过 FIFO 容量则断开远端并明确报错，本机继续；不偷偷丢帧或用重采样追赶。没有可用声卡时，由远端时钟推动渲染。主机渲染暂时不足也等待完整样本，不用伪造的零样本填掉缺失的节目。

接收器在欠载后积累至少 40 ms 再继续原样输出；等待期间设备收到静音，不对恢复后的节目样本做淡入。纯远程输出时主机暂停保留其待发送 FIFO；双端模式主机暂停按本机规则同步清理缓冲，暂停的听觉效果会受到网络缓冲延迟影响。换曲 / 重播使用有序 reset 包清掉旧曲缓存，旧数据不能越过 reset 重新播放。

PCM 模式超过 15 秒无响应会断开，HLS 使用媒体请求租约。本机仍按当前播放状态继续；用户可重新连接远端。

## 实现与安全

- `apps/desktop/remote-session.cjs`：TLS 加密会话、双连接仲裁、流量额度、命令白名单、限长帧解析、原生进程生命周期。
- 使用 Node/Electron 内置 TLS（最低 1.2，支持时使用 1.3）；未填写自定义密钥时，每次开启发送生成 256-bit 随机配对密钥。P-256 自签名证书保存在用户数据目录，重启复用，临近过期自动更新。原生配对地址携带 SHA-256 证书指纹，原生客户端在发送密钥前严格校验指纹。网页由浏览器验证 HTTPS 证书，首次访问需要用户确认自签名证书；应用不关闭浏览器安全验证。主机认证密钥后才发送音频或状态。配对地址相当于访问密钥，不应公开；不写入主机设置或应用日志。网页从 URL 片段读取密钥后移除片段，仅在该浏览器标签的 sessionStorage 中保留，用于刷新重连。
- `remote-web-server.cjs` 与 `remote-web/`：同一 TLS 端口通过 ALPN 分发 HTTPS / 原生协议；网页静态资源白名单、同源 WebSocket、首包认证，与原生共享唯一接收槽。网页资产及 ws 依赖随 Electron 打包，离线局域网也可加载。
- 主机原生音频与 Electron 之间使用一次性随机 token 验证的 loopback TCP，不向局域网暴露原生控制管道。
- 控制协议只能操作已有播放会话，不能传任意磁盘路径、执行代码或打开系统文件。网络状态不包含本地文件路径和文件句柄。
- `remote_audio.rs`：代替主机 WASAPI 的单一 FIFO 消费者，以及绕过渲染图的客户端 PCM 入队。
- 接收器已包含在现有 `SdaNativeRenderer.exe`；Electron 打包清单包含新会话模块，常规打包流程即可分发。

## 当前范围

远端可使用网页或 Windows SDA 原生客户端，二者合计只能连接一个。不提供多人房间或公网中继。播放进度显示主机时钟；现有播放器尚无随机 seek 接口，所以不提供假的拖动进度操作。网页包含房间、监听和耳廓面板；对象 3D 主视图、房间声路动画/布局记忆、文件导入导出仍在主机。

## 验证

- Node 集成测试：真实 TLS loopback 配对、错误密钥、证书指纹错误、第二客户端拒绝、重连、控制确认、状态中不暴露路径、有界接收额度，逐字节对照 PCM。
- 帧解析测试：拆包 / 粘包、多字节文本、超长帧拒绝。
- Rust 测试：最终 FIFO 样本包括负零原样序列化、无额度不消费、渲染欠载不消耗额度、有序 flush、拒绝非 loopback 原生地址、接收欠载恢复不淡入也不丢节目样本。
- 原生回归：121 项通过，8 项离线专项跳过；TypeScript 与生产构建通过。
- 实际 Windows 验证：主机指定不存在的声卡仍能远程发送；Realtek 接收端共享 48 kHz / float32、独占 48 kHz / PCM16 均能收听。Electron 分别作为主机和接收机验证通过，控制往返及界面无异常。该声卡实际独占格式为 16-bit，界面如实显示，不将其标成 32-bit DAC 输出。
- 网页验证：HTTPS 静态资源与访问边界、错误密钥/来源拒绝、浏览器和原生互斥、PCM 逐字节一致、重缓冲与 reset 额度、持久证书；Edge 实际 AudioWorklet 播放、控制确认、暂停浏览器音频后有界背压、恢复、断开、深浅主题和窄屏布局。Electron 主机到 Edge 网页实际出流与音量控制通过。Safari / Firefox 和真实异地线路尚未实测。
- 网络测试不代替真实异地线路的延迟 / 丢包测试；具体网络缓冲按用户线路选择。

## 远程桌面与电源

发送服务不依赖 UU / RDP 会话；锁屏、关闭显示器和本机声卡变化不应结束发送。远程模式期间使用 Electron prevent-app-suspension 防止自动空闲休眠，结束后释放，不强制屏幕常亮；用户主动休眠、关机和网络断开仍会中断连接。日志记录远程角色/状态变化、显式停止请求和系统锁屏/休眠事件，不记录配对密钥。

开发验证必须使用隔离实例；不得在用户正在使用的主机上执行自动停止/重启清理。

## 自定义配对密钥

发送端可填写最多 256 个字符的自定义密钥，首尾空白忽略，留空或仅空白则每次随机生成。文字密钥使用 PBKDF2-SHA256（210000 次，协议固定盐）转换为 32 字节配对凭据；64 位小写十六进制可直接作为凭据。网页打开主机 HTTPS 地址后可输入自定义密钥，也可继续使用带凭据的完整链接；SDA 原生客户端继续使用完整配对地址。原始自定义密钥不写入设置和日志。

固定密钥再次使用时配对凭据相同，因此同一主机端口的旧网页链接也可再次使用；要撤销旧链接请更换密钥或留空使用自动生成并保存的密钥。停止发送期间所有链接均无法连接。

## 手机后台播放与原生无损 HLS

网页默认检测原生 HLS 支持，使用 HTMLAudioElement 播放同源 HTTPS 的 FLAC/fMP4 直播分段。Safari 的媒体管线自行请求分段，不依赖网页定时器、WebSocket PCM 或 AudioWorklet 持续供音。原生 HLS 不可用时使用原有 float32 PCM；连接页也能主动选择低延迟 PCM。原生 HLS 播放失败会明确报错，不静默回退有损编码。

- HLS 格式为 48 kHz、双声道、24-bit FLAC。主机 float32 按 round(x * 8388608) 转换为有符号 PCM24，并限幅到整数范围。FLAC 对这一整数结果无损，**不是 float32 位完美**；不加响度补偿、增益、抖动或重采样，超满幅样本计数会显示。编码使用 FLAC verbatim 子帧，无额外编码器运行时，约 2.32 Mbps 加 HTTP/TLS 开销。
- 每个分段最长 1 秒，至少准备 3 段再连接；播放列表保留最近 10 段，内存最多保留 16 段，生产节奏最多领先墙钟约 4 秒加原有信用窗口。实际延迟为数秒，由浏览器缓冲决定，不套用 PCM 模式的 100–1000 ms 延迟承诺。换歌、调节房间/监听等主机变化也需经过该缓冲；进度显示是主机位置。复播离直播边缘太远时回到近直播位置。
- HLS 和网页 PCM 共用两个收听名额。首次配对需原密钥、同源 Origin 和电脑批准；之后用每台设备独立的 HttpOnly/Secure/SameSite=Strict Cookie 凭证，主机只保存凭证哈希。分段还需匹配该设备的 HLS 会话 Cookie。媒体 URL 不含配对密钥。支持独立分段的普通字节范围请求。
- WebSocket 只负责控制和状态，断开会重连，不终止原生音频。系统媒体操作在控制连接暂时断开时走同源鉴权 HTTP，并等待主机确认。显式断开走 HTTP 停止接口，可在 WebSocket 失效时释放名额。关闭页面会断开；仅切后台不主动断开。
- 收到媒体请求更新 90 秒租约。前台暂停/耳廓测试也可通过控制心跳维持租约；背景音频继续请求分段即可维持，无须网页 JS 心跳。系统回收页面、网络中断或暂停后不再请求超过 90 秒则释放客户端，远端退出而本机继续播放。
- Audio Session 在可用时设置 playback，Media Session 提供歌曲信息及播放控制。PCM 路径仍受后台 AudioContext/JS 暂停与 15 秒超时限制。耳廓测试在 HLS 模式暂停媒体元素，结束后恢复之前的播放状态。

实现参考 Apple《HLS Authoring Specification for Apple Devices》音频格式要求：Apple Lossless / FLAC 使用 fMP4。这里提供支持该无损格式客户端的单音轨直播，不提供规范中面向广泛兼容性的 AAC 备用轨，不宣称所有 HLS 客户端兼容。HTTPS 自签证书仍需要设备信任；媒体加载器也必须接受该证书。

验证：独立 FFmpeg 解码器读取生成的 fMP4/FLAC，400 帧共 192,000 样本/声道与量化后的 PCM24 逐样本一致，覆盖满幅、越界及跨 FLAC 帧编号；Node 集成测试覆盖鉴权、独占名额、控制断线重连、HTTP 控制/断开、内存上限、重置与租约过期。16 项远程协议测试通过。桌面 Edge 原生 HLS 实际播放及控制重连连续播放已验证；隔离 Electron 的真实原生渲染器空闲流、网页监听参数应用及 390px 窄屏也通过。**没有 iPhone/Android 真机，未验证移动端锁屏长期播放；桌面测试不替代此验证。**

参考：
- https://developer.apple.com/documentation/http-live-streaming/hls-authoring-specification-for-apple-devices
- https://developer.mozilla.org/en-US/docs/Web/API/AudioSession/type
- https://developer.mozilla.org/en-US/docs/Web/API/Media_Session_API
- https://bugs.webkit.org/show_bug.cgi?id=261554

## 网页房间、监听与耳廓

播放器下方的三个按钮打开可关闭、可滚动的悬浮面板，深浅主题与窄屏共用。除耳廓感知测试音外，所有音频处理仍由主机执行。

- 房间：选择主机内置/自定义档案（限制为当前布局）、启停房间、直达/早期/完整反射试听、反射电平与分界、各音箱校准。可填写尺寸、材料和反射阶数由主机生成新房间，并查看进度或取消；生成后从档案中选择应用。
- 监听：独立开关、电平、DIM、静音、各通道 trim/延时/反相/静音、低频管理、当前房间对齐、内置监听配置和 AHB2 参数配置，以及全部硬件链路数值。
- 耳廓：切换主机已保存个人档案和内置测量库，可命名当前个人档案或另存副本，KU100 数据校准与高解析逐对象 HRTF。手机感知测试复用桌面算法与实际主机布局，每音箱确认及移动确认完成后在主机生成、保存并应用个人档案；测试可取消，生成失败可重试。测试期间暂停网络 PCM 输出，防止旧节目缓冲混入本地测试音。

房间和监听应用携带编辑前快照，主机若已改变对应配置则拒绝覆盖，提示撤销草稿并重新编辑。改变房间时保留当前监听，改变监听时保留房间。命令使用明确白名单与同一后端数值校验，网页状态不含文件路径、音频响应或任意系统接口。常规操作主机确认后才显示已应用；房间生成使用与主机十分钟生成上限匹配的操作超时及可并行取消。

验证：14 项协议/认证/音频/控制参数测试通过；隔离 Edge 实测房间、监听、耳廓切换，完整 2.0 定位与移动感知流程，并提交生成参数；隔离 Electron + 真实原生渲染器验证监听电平、房间启用和反射参数、D2 切换及 390px 悬浮面板无横向溢出；2.0 感知测试实际生成个人档案并由主机应用。房间生成已验证任务启动及通过网页取消，取消后收听连接保持正常；现有仿真器这次运行超过两分钟未完成，不声称生成耗时已优化。未操作用户正在播放的 Electron 会话。定位测试不等于验证听者主观效果，iOS/Android 仍需真机验证。

## 移动网页媒体选择器

播放器的“打开主机媒体”入口只列出主机已保存的收藏目录和最近目录。可浏览其子目录、单曲加入播放列表、整目录加入播放列表、返回和关闭；沿用主机播放列表去重与空闲时自动播放规则。不提供盘符/主目录列表、任意路径输入、收藏增删或清空最近记录。

浏览器仅收到随机化的不透明项目 ID、名称和目录标记，不收到真实路径。主机每次读取或打开均重新检查项目仍属于收藏/最近目录，并通过 realpath 限定实际位置，拒绝跳出目录的链接。目录数量、返回包大小和递归选曲数量均有限制。17 项远程测试及 TypeScript/生产构建通过。

播放状态同步修复：主机的 paused/playing 状态同时驱动网页 HLS 音频元素和 Media Session，而不只更新按钮。主机暂停/停止时本端 pause，主机恢复时尝试 play；浏览器要求手势时保留恢复入口。连续进度广播不重复调用 play，也不强行恢复本地中断；耳廓测试结束按最新主机状态恢复。桌面 Edge 实测主机发起暂停/继续，媒体元素、按钮和 Media Session 同步通过。若 iOS 冻结网页 JS，状态处理仍须等待网页获调度，不能保证锁屏时即时响应。

## 封面与滑动页面

网页收听区域使用随软件打包的 Swiper 12：正在播放与播放列表分成两页，支持触摸横滑、轮播区域聚焦后的左右键和底部分页标记；不显示顶部标签。音量等控件不触发横滑，列表仍可纵向滚动；非当前页面禁止焦点进入，减弱动态效果设置会关闭切页动画。

桌面当前歌曲的嵌入封面生成最长边 384px 的 JPEG 缩略图（小于 180KB）。状态消息只包含封面内容哈希；客户端在封面变化时通过已认证控制连接按需获取一次图片，并同步到 Media Session。无封面、换到无封面歌曲或解码失败时使用默认图形；旧歌曲的异步图片响应不会覆盖新歌曲。不向手机发送本机路径或 blob URL，不访问第三方封面服务器。缩略图不是原图字节透传。

19 项远程协议/控制测试、TypeScript 检查和生产构建通过。桌面 Edge 的 390px 触摸事件测试覆盖分页切换、列表选曲、图片显示/清空及 Media Session 封面；移动系统锁屏展示依然取决于系统支持。

真实链路补充验证：隔离 Electron 打开带嵌入封面的 M4A，经桌面缩略图生成、认证连接传输后，移动网页成功显示封面并写入 Media Session；选曲、断开清理和 390px 布局通过。

## 渲染输出取样核验

针对手机听感疑似原始音频的反馈，新增 Rust 回归测试 `remote_sink_sends_final_hrtf_output_not_source_pcm`：输入仅左声道有信号的 PCM，用真实校准 HRTF 和最终耳机 FIR 渲染，再经 StereoFifo 与 HostOutput 的真实 TCP 出口读取 4,800 帧。远端收到的数据与 render_into 的最终双声道结果逐位相等，且右耳包含 HRTF 交叉响应，区别于原始右声道静音。

将上述真实渲染采样送入网页使用的 FLAC/fMP4 编码器，用独立 FFmpeg 解码后，PCM24 逐样本一致；最大 float32→PCM24 量化绝对误差 5.960464477539063e-8。验证覆盖发送点与编码器，不覆盖 iPhone 系统或蓝牙耳机内部的处理，不能据此宣称已定位用户的听感差异。

代码链路为 render_into（HRTF/房间、耳机 FIR、EQ、主音量、峰值保护）→ StereoFifo → HostOutput → TLS PCM 或 FLAC/HLS → 浏览器。两种远程模式均不读取或向客户端返回歌曲文件。纯双声道节目仍遵循主机原始/干双耳/房间选择；含对象或多声道节目不受纯立体声原始模式选择影响。

## 同时输出与重新连接修复

声卡可用时保留 WASAPI 输出，原生渲染工作线程将同一最终输出分别写入本机 FIFO 和独立远端 FIFO；远端消费者不再消耗本机 FIFO 或累计本机播放时钟。远端 FIFO 溢出只中断远端，不能阻塞本机。声卡缺失时仍支持纯远程消费主 FIFO。初始 reset 包必须等待客户端 H 就绪握手后才发送。

关闭监听等待端口释放；新会话等待旧原生输出清理完成。同一浏览器重连会撤销旧 HLS 会话 ID，迟到的旧 stop 请求不能关闭新会话。自动随机密钥在本机设置中保存，重新开启/重启不再自动更换；需要撤销旧链接时设置新的自定义密钥。网页快速断开重连会先等 HTTP 停止完成。

验证：浏览器连续三轮断开重连、主机关闭再开启后原密钥重连通过；真实 Realtek 声卡与原生远端同时输出，通过断开远端/关闭发送后检查本机播放时钟持续前进；无可用主机声卡时纯远程回归通过。

### 电脑端静音

电脑远程设置与网页播放卡片提供同一个“电脑端静音”开关，默认开启并保存在主机设置中。仅在远程音频通道连接期间生效，断开后恢复本机播放。手机修改后同步回电脑。静音位于 WASAPI 输出末端，使用短淡入淡出，不改变渲染结果、远程 PCM/FLAC、播放时钟或主音量。关闭后允许电脑与手机同时出声（仍有网络缓冲时差）。

自定义配对密钥在成功开启发送后保存于主机设置，下次打开设置自动回填，省略密钥的重新开启操作也会复用。显式清空后开启发送会清除保存的自定义密钥，切换到自动密钥。明文自定义密钥只通过本机设置 IPC 读取，不添加到远程状态广播。


### 手机空间视图

Swiper 第三页复用桌面 ObjectView 组件，展示主机当前音箱布局、对象位置和大小、发声与静音状态。单指旋转，双指缩放/平移；通过画布之外的标题/说明区域滑动切页。按需加载 3D，第三页可见且网页前台时串行请求最新快照（约 10 Hz），本地平滑绘制并限制到 30 fps；离页或后台卸载画布，音频不受影响。场景数据上限为 256 对象和 64 音箱，不包含任意主机路径。视图是主机实时状态，尚未按手机 HLS 播放缓冲作音画对齐。


## 双设备授权与状态同步

- 主机保存已授权设备名称和权限，可单独断开、撤销或改变权限。权限改变会断开该设备，重连时取得新权限。撤销立即失效，重新使用共享密钥也必须重新请求批准。
- 首次配对每个来源一分钟最多 5 次尝试，待审批最多 4 个，申请 2 分钟过期，最多记住 16 台设备；同时收听仍限制为 2 台。设备凭证为随机 256-bit，Cookie 有效期一年。
- 主机一次取流，按设备分别维护确认序号和发送队列。一个设备积压超过 6 秒或失联，只断开该设备。HLS 的媒体会话、Cookie、控制通道、片段缓冲均隔离，不能使用另一设备的 URL 或 Cookie 跨会话控制。
- 状态带主机递增版本号，播放、暂停、曲目、列表、循环、房间、监听、耳廓、主机音量与电脑端静音广播到所有设备；新连接立即获得完整状态，旧版本不覆盖新状态。两台设备的修改按主机接收顺序执行，取消生成不被正在运行的生成命令堵住。
- 「主机音量」仍是所有设备共享的渲染增益，各设备耳机的收听音量通过自身系统控制。状态同步不等于设备间采样级同步出声，HLS 缓冲延迟仍独立存在。
- 新模块 `remote-devices.cjs` 实现授权和限速，`remote-fanout.cjs` 实现一次取流到多设备的有界发送。两者已列入桌面打包清单。

验证：30 项远程测试通过；两个独立浏览器实际走 HLS 完成配对批准、同时播放、循环/电脑静音/暂停同步、凭证重连和单端断开；真实原生音频同时送到两台测试接收器，并保留本机播放。没有 iPhone/iPad 真机，未以桌面浏览器结果替代 iOS 后台播放验证。

## 高对象数播放与缓冲

- 原生远程发送每次调度最多处理 16 次非阻塞收发，短暂调度延迟后可补发已渲染的积压音频；没有进展立即退出，不等待网络、不补造音频、不改变采样。
- 网页 PCM 的启播/重新缓冲阈值采用所选网络缓存，预留 40 ms 反馈余量，不再固定为 40 ms。1000 ms 档对应约 960 ms 接收预缓冲。
- Safari HLS 保留原有 1 秒片段、至少 3 秒媒体准备；毫秒选项控制传输窗口，不等于 Safari 的播放缓存。较长 HLS 缓冲实验在 Edge 暂停恢复时出现解复用错误，未启用。HLS 取流超前量降为 1 秒，防止第二设备建连时挤爆第一设备队列。
- 这些措施覆盖发送调度抖动及接收缓冲不足，不能修复上游长期低于实时速度的解码/渲染。仍需在实际 iPhone 和 108 对象曲目上复测，不能把测试机结果当作真机不卡顿的证明。

补充验证：本次 35 项远程单元/集成测试、5 项原生远程测试及真实原生双接收器测试通过。Edge 高缓存双浏览器测试在暂停恢复后仍可出现 DEMUXER_ERROR_COULD_NOT_PARSE（恢复原有 HLS 分段后仍复现），尚未定位；不能宣称浏览器端所有卡顿/断流均已解决。尚无此次 108 对象曲目的 iPhone 真机复测。

## 起播状态

主机远程快照直接读取播放器实际消费采样位置，并用 hasStartedOutput 标识起播是否完成；不使用 3D 的预测显示时钟。点击播放、构建解码器、填充启动缓冲期间发送 loading=true。网页优先显示加载状态、冻结进度、暂停 MediaSession 计时；空白 HLS 流处于 playing/canplay 不能覆盖主机加载状态。曲目结束由主机发布，网页不以 MediaSession 的显示时长触发停止。

## 108 对象音源欠载修复

2026-09-11 实际日志：118 source、输出 FIFO 欠载为 0，但 sourceUnderrun 持续增加；多批 4096 采样的真实音频被返回 `stale replay accepted idempotently`。旧 render worker 在解码 PCM 缺失、输出 FIFO 尚有旧音频时继续生成静音并推进 codec 时钟，导致迟到音频永久丢弃，接收端加缓存无法弥补。

新增 PCM 时间范围记录，只允许渲染实际成功写入的连续音频区间；缺整批数据时等待，最后不足一个分区的真实采样按实际长度渲染。显式静音样本照常推进，单个对象不发声不触发全局等待。64 路以上的原生流起播预缓冲从 0.5 秒提高为 1.5 秒，保留全部对象和 HRTF 处理。

复现实验 `node scripts/test-native-source-starvation.cjs`：118 路只送 24576 采样，延迟下一批。旧版输出位置误跑到 72960，产生 7612416 音源欠载采样；回归断言应停在 24576，迟到的下一批继续播放到 49152，欠载计数为 0。

验证结果：新版 118 路延迟供给实验输出 position=24576/49152，两个阶段 underruns=0；原生完整测试 126 通过、8 跳过；真实原生双远程接收器仍能同时收听，单端断开及停止发送后本机继续播放。Web 类型检查和生产构建通过。

## 108 对象持续供给与提交事务

后续现场日志显示：音源欠载已归零，但输出 FIFO 仍反复耗尽。不能将“不再丢解码样本”当作“播放已不卡顿”，也不能靠接收端增加网络缓存修复主机欠载。

- 同一解码帧按顺序提交。稳定播放阶段将对象事件和 PCM 合并为一个原生事务、一次 IPC 和一次真实 batch ACK；首帧/新增声道仍先提交元数据，再完成源声明。保留全部对象、位置、宽度与既有双耳处理。
- 原生 `F` 帧携带有界 JSON 事件数组与 PCM 批次；进入渲染队列后先验证事件/样本/源容量，再应用事件并提交 PCM。旧 `B` 帧继续兼容。整批拒绝不会提前写入新的对象事件。
- 管道 drain 不再批量假定成功；每个等待帧保留自己的元数据，并等待自己的后端 ACK。重复的待确认批次复用同一个 Promise，停止时逐批拒绝。
- 连续对象快速路径按对象连续读取已经完成的输出块再求和，减少逐采样跨 108 个对象缓冲访问。可能在块内更换卷积器的通用路径保留原逐采样读取，不改变事件生效时刻。

验证：传输队列 8 项测试通过；118 源延迟供给测试同时覆盖旧 `B` 与新 `F` 通路；原生双接收器、本机继续播放回归通过。原生音频测试 126 项通过、8 项依赖额外资源/性能基准而跳过。

实际母版供给基准采用 NaturesFuryADM（10 床层 + 108 对象）、当前 433 方向个人档案、48 kHz：完整 108.667 秒期间音源欠载为 0、播放中输出欠载为 0；最小提交前瞻 3.913 秒，平均渲染块耗时 12.347 ms（预算 21.333 ms）。最终停流后的 FIFO 自然耗尽另计，不能混作播放中欠载。此基准为原生流测试，不替代实际 iPhone/Safari 端复测。

可复测步骤：先用 `scripts/prepare-adm-native-benchmark.mjs` 生成本地母版元数据，再运行 `scripts/test-native-adm-stream.cjs`。支持 `SDA_ADM_BENCHMARK`、`SDA_HRTF_SET`、`SDA_PERSONAL_HRTF_ROOT`、`SDA_TEST_EXE`；设置 `SDA_ASSERT_REALTIME=1` 会断言播放过程无 FIFO 欠载。不要与编译或其他重负载基准同时运行性能测试。

### Electron 界面阻塞与文件预读

原生基准通过后，Electron 实播仍复现低前瞻。CPU 采样找到 `useRef(readOutputLatencySeconds())` 在每次 React 刷新时求值，通过 `sendSync` 同步读设置：12 秒采样中约 3.3 秒阻塞在此调用；解码 Worker 反而约 11.1 秒处于 idle。已改为惰性初始化，只在组件初始化读取，后续保持可更新的延迟 ref。

桌面文件分片改为异步文件 I/O，避免主进程同步 open/read/close 挡住音频 IPC；读取与解码之间仅预读一个分片，保持有界内存、原始顺序和既有播放节流。取消会等待未完成的预读清理，预读错误不会成为未处理的 Promise rejection。3 项预读回归测试通过。

2026-09-11 新版 Electron 使用当前个人档案、房间开启、监听及硬件关闭，实际播放 118 源至 sample=2937856（约 61.2 秒）后暂停：sourceUnderrun=0、fifoUnderrun=0，46 个播放中快照的最小前瞻为 2.689 秒，暂停后回填到约 4 秒。修复前同一桌面路径的前瞻曾降到 0，输出欠载累计 1345888 帧。保持现场暂停状态，没有替用户继续播放；手机端主观收听仍需实际连接确认。

### Tailscale / Safari 未播完切歌的媒体隔离（2026-09-11）

每次原生音频重置创建独立 HLS media epoch，丢弃旧片段及未完成片段，旧 epoch 地址返回 410；PCM 信用计数保持累计，避免重置造成流控死锁。手机点击列表歌曲或上一首/下一首时立即暂停并卸载旧 HLS 音源，PCM 通路则清空并暂时保持静音，等待有序 reset。

新 HLS epoch 至少产生 3 个真实音频片段后才装载。等待期间显示加载、进度保持零；实际播放后使用媒体时间加当前 epoch 的歌曲偏移显示进度，不再提前显示主机游标。这不会消除 Tailscale 传输与 Safari 缓冲所需的等待，也不承诺手机与主机采样级同步。

28 项远程回归测试通过，包括旧地址拒绝、切歌清缓存、晚到旧 epoch、连续切歌、重置先于曲目状态、失败恢复、播放进度、多设备及缓冲设置。实际 iOS / Tailscale 听感须在更新后刷新网页复测。

### Safari / Tailscale 卡顿诊断（2026-09-11）

HLS 页面通过已认证控制连接每秒最多上报一次当前 epoch、实际媒体时间、连续缓冲余量、readyState、等待/暂停/切歌及页面隐藏状态。不传音频或媒体路径。服务端校验 epoch 和数值范围，正常每 5 秒、等待时最多每秒记录一次 `remote-receiver`，同时记录主机播放状态、最后 PCM 距今时间、采样周期内最大 PCM 到达间隔与已生成片段时长。诊断不参与 PCM 信用计数，不调节音量或音质。

日志位于 `tmp/sda-startup.log`，按时间与原生 renderer health 对照。主机供给中断和接收端缓冲耗尽可同时存在；主机持续供给、手机缓冲归零只提示传输或浏览器问题，不能直接归罪于 Tailscale。iOS 后台可能暂停网页 JS，缺失上报不能视为没有卡顿。主机暂停/加载及 epoch 重置会清除供给间隔基准，避免将正常暂停算作供给故障。

本次现场无收听连接、原生播放器未启动。活跃 iPhone 的 5 次 Tailscale ping 均经公网端点直连，65–78 ms；status 中 Relay 字段本身不证明正在走中继。此次探测不是带宽测试，也不能排除播放期间的瞬时抖动。29 项远程回归测试通过，仍待真实歌曲卡顿现场关联日志。

### 同步要求与方案撤回（2026-09-11）

用户明确要求电脑和手机同步开始、暂停、结束，不接受手机独立播完尾音或仅修改显示进度作为解决方案。尝试中的 `drainRemoteAudio` / HLS 尾音等待改动已撤回，未部署。当时版本尚未实现该同步要求；后续实现见下节。

本次复现（UTC 10:19:08–10:19:20）：手机连续缓冲降至 656 ms、280 ms，而服务端已生成 98、104 秒音频；主机 PCM 到达间隔最大约 0.5 秒。主机结束后手机停在 99.985 秒、尚有 8.015 秒缓存。原生现场 health 为 118 source、0 underrun。前半段证据指向传输/浏览器消费跟不上，不能单独认定 Tailscale 故障；后半段明确暴露两端独立播放时钟以及主机结束状态提前停止手机的问题。

满足要求的调度必须包含：源样本位置和媒体 epoch；客户端与主机时钟偏移/往返延迟测量；全部接收端就绪屏障；共同的未来起播时刻；WASAPI 本机输出门控（预渲染与实际播放分离）；运行期偏差测量和明确的误差限；缓冲不足时共同暂停和重新同步；暂停、切歌、曲终沿同一时间线执行。测试必须测实际输出，不能仅比较进度条。

平台约束：当前 Safari 原生 HLS 只通过 HTMLMediaElement 控制，其 play() 不提供按共同硬件时钟预约输出的接口。网页前台可以做基于时钟测量的近似协调，不能由此声称锁屏后台严格同步。苹果原生 AVPlayer 的 setRate(_:time:atHostTime:) 支持把媒体时间映射至指定 host clock，且官方强调调用前仍须自行准备媒体数据；AVPlaybackCoordinator 为原生播放器提供组播放协调。这些接口不是 Safari JavaScript API。若要求保留 iOS 后台并获得可控的播放时钟，需要原生接收端，再配合跨设备时钟协议与输出延迟测量；不能偷偷改为有损链路、删除后台播放，或把固定延迟当作同步。

核对的苹果官方资料：
- https://developer.apple.com/documentation/avfoundation/avplayer/setrate(_:time:athosttime:)
- https://developer.apple.com/documentation/avfoundation/avplaybackcoordinator

### 共同起播与输出门控的实现（2026-09-11）

本次实现覆盖电脑原生输出与网页 HLS 接收端（包括两台 HLS 设备）。同步控制实际消费者，而非延迟进度条，也未采用已撤回的曲尾等待方案。

- `remote-sync.cjs` 负责就绪屏障、时钟协商和共同起播。浏览器测量控制连接往返时延与时钟偏移；所有接收端在同一源样本处缓冲就绪后，预约同一个未来时间点。手机未就绪时，电脑 WASAPI 不消耗 PCM，进度不会先走。
- 原生 `remote_sync.rs` 将预约时刻转换为单调时钟，由输出回调执行门控。同步命令在协议线程直接处理，并使用独立控制队列，避免排在耗时的房间/卷积配置之后；已错过预约窗口的起播请求拒绝执行。
- 预渲染与播放分离：同步时本机最多预渲染约 6 秒，播放器最多前瞻 8 秒。仅在镜像接入时复制本机尚未消费的 FIFO，并携带源样本位置，解决歌曲中途首次连接和第二台设备加入的时间偏移；所有缓冲均有界。
- 接收端缓冲接近耗尽、持续漂移或控制连接中断时，共同暂停并在本机实际样本位置重新就绪。正常暂停不再丢弃原生音频 FIFO；已排队的 PCM 不因房间/耳机 DSP 更新被清掉，设置作用于尚未渲染的部分。
- 原生 `M`、`N` 消息只在已认证的本机代理连接上传递源起点和精确结束样本。最后不足 10 ms 的 PCM 保留并补零封包，HLS 封闭最后一个不足一秒的片段并写入 ENDLIST；没有丢掉原先遗漏的曲尾。原生曲终判断等最后 ACK 和最后实际消费，不再提前 200 ms 结束。
- 原生对象活动队列扩至覆盖同步前瞻窗口，避免音频门控后对象高亮却丢失前几秒。

自动验证包括 34 项网页/传输回归、129 项原生全量回归（8 项原有离线基准忽略），以及后续补充的同步命令绕过 DSP 队列测试。`scripts/test-native-remote-sync.cjs` 使用真实 WASAPI 与真实浏览器 FLAC 解码及媒体时钟，验证首次起播、中途加入、暂停恢复、人工阻断下载后的共同等待、第二设备加入和曲尾。Windows Edge 的原生 HLS demuxer 拒绝 FLAC，因此该联调通过仅用于测试的 MSE 适配器输入相同 fMP4 字节；没有伪造音频时间、readyState、play 或 pause，适配器不打包到软件。

实测：中途连接的双声道用例，三个时钟检查点偏差为 12、-53、-5 ms；108 对象母版片段（118 源、逐对象 HRTF），最终复测偏差为 -49、-28、-26 ms，音源欠载为 0。原生终点 9.507 秒，浏览器终点约 9.510 秒（包含最后 PCM 包的补零）。这些是媒体时钟检查，不是麦克风测得的两副耳机声学输出延迟。

复测命令：提供可用的 Playwright 模块路径 `SDA_PLAYWRIGHT_MODULE` 后运行 `node scripts/test-native-remote-sync.cjs`；`SDA_SYNC_JOIN_DURING_PLAY=1` 覆盖中途首次连接，`SDA_SYNC_ADM_FILE=tmp/adm-performance.json` 使用已有母版基准数据。测试使用临时本机端口及独立浏览器上下文，不写用户配对设备或密钥。ADM 用例将测试原生输出音量设为零，避免测试打断实际收听。

边界：Safari 实机、iOS 锁屏调度、蓝牙耳机输出延迟尚不能由 Windows 联调证明。网页仍不是硬件采样时钟锁定；当前持续偏差超过 300 ms 三次会重新同步，浏览器错过起播预约超过 100 ms 会拒绝迟到起播并请求重新同步。传统 PCM/native 接收端尚未加入这套 HLS 就绪屏障，不能把混合接收模式宣传为已同步。部署后旧网页须刷新，才能加载新的同步协议与脚本。

### 2026-09-11 Safari startup investigation

The user clarified that the audible output was UU Remote, not the web receiver; Safari recovery is **not verified**. Receiver logs showed `readyState=4`, `time=0`, `aheadMs=0` while the native consumer correctly held at sample zero with six seconds produced. Those logs did not include TimeRanges, so they do not establish whether this was a timestamp gap, live-edge buffering, or Safari preload behavior.

The web receiver now logs bounded buffered/seekable ranges and the requested source position. Readiness and low-buffer feedback share a 20 ms timestamp tolerance, without treating genuinely missing audio as buffered. Synchronized HLS explicitly requests the beginning of the advertised playlist with EXT-X-START rather than Safari's default live edge. Repeated play requests no longer invalidate an in-progress readiness barrier. Startup reserve (1–3 seconds) and the common start deadline (1.2–4 seconds) adapt to measured control round-trip time. This does not guarantee uninterrupted lossless playback when sustained network throughput is insufficient.

Validation: 23 remote JavaScript tests passed, including timestamp-gap/missing-audio distinction, slow-tunnel readiness/deadline, and repeated-play barrier preservation. Electron restarted with pairing retained. Actual iPhone loading and playback still require receiver feedback from the refreshed web page; no iOS success is claimed.

Follow-up: the refreshed iPhone did report a valid [0,1] buffered range and [0,4] seekable range. After a rejected start, the held native position was 1.45 seconds. The receiver refused to seek there because it was not yet buffered, preventing Safari from requesting the required segment. Preparation now seeks within the advertised seekable range and continues withholding readiness until actual data is buffered. No seekable range is counted as downloaded audio. Scheduled browser timers also use a fixed monotonic deadline, so a later clock-offset estimate cannot falsely label an on-time callback late. Added regressions for both cases; all 13 sync/playback tests pass. Actual iPhone restart still needs confirmation; the first rejected start now records its arrival delay and timer lateness.

Follow-up on repeated 1–2 second stalls: logs showed repeated clock-drift holds with 4–5 seconds still buffered and start timer lateness of only 0–2 ms. The controller previously retried identical start timing despite persistent media-clock lag. It now uses current-revision running feedback after a two-second startup settling interval and learns a bounded per-receiver startup lead from early measured skew for the next coordinated start. The actual HTML media play call is scheduled earlier by that lead; native playout still uses the common deadline. This compensates measured startup lag; it does not promise sample-exact Safari hardware output or bypass genuine starvation. Exact seekable endpoint requests are also allowed, without treating them as buffered data.

Validation: 15 sync/playback tests passed, including startup-lag feedback and stale revision rejection. Real WASAPI + Edge FLAC/MSE integration passed start, pause, resume, network starvation hold/recovery, two receivers, and final sample completion (tmp/sync-safari-lag-proof.log). This integration is not native Safari HLS. Electron redeployed, iPhone behavior remains to be verified.

Pause-state fix: iPhone logs confirmed uninterrupted playback to about 42 seconds, followed by an intentional pause. Both consumers stopped, but hostLoading remained true because the desktop treated a held synchronization gate as loading even while paused. Desktop publication now excludes paused state from loading; the server also normalizes paused/loading consistently. Receiver presentation distinguishes an explicit pause from a pending new play request and preserves elapsed position across synchronization waits. A pause hold clears stale pending-start state. All 21 receiver-view/sync/playback tests passed, including pause with stale loading flags and elapsed-position retention.

Remote sound-setting feedback: room/monitor/HRTF acknowledgements now distinguish host application from delayed audible change through pre-rendered audio. The tools panel shows an explicitly estimated wait based on the media cursor (host/receiver position plus an eight-second conservative rendering allowance), and pauses the countdown when playback stops advancing. Paused receivers are told to continue playback for cached audio to clear; track changes discard the old estimate. This is not a sample-tagged DSP boundary and must not be presented as exact confirmation of the audible transition. Settings changes do not flush buffered sound or interrupt synchronized playback. Added regression coverage for countdown, pause, completion and track replacement.

Paused receiver prefetch: the web client may proactively fetch up to eight seconds of upcoming already-rendered HLS segments while explicitly paused, without playing or seeking the audio element. Downloads are sequential and bounded, abort on resume/disconnect/epoch replacement, and time out after ten seconds. Successful authenticated immutable media responses allow private 30-second HTTP caching; playlists, controls, errors and credentials remain no-store. Cached segment refresh is bounded; this is HTTP prefetch, not a claim that Safari has decoded the data. iOS may suspend page tasks in the background or use a separate media cache. No whole-song download or paused DSP re-render is implemented. Pause stays pause and its connection indicator can show prefetch activity. Fifteen prefetch/web-server tests passed.


### Native background playback and low-latency HLS

The receiver continues to use the system HTML audio HLS player. No default switch to an AudioWorklet or JavaScript-driven MediaSource pipeline is made; iOS lock-screen playback remains on the original native media path. The server now publishes 200 ms independent FLAC/fMP4 parts, blocking playlist reloads and preload hints, while retaining complete one-second segments for ordinary HLS clients. Full segments concatenate exactly the same parts, with continuous decode timestamps. Authenticated part requests keep the session lease alive independently of page JavaScript.

The host starts with a six-second render reserve. Only after every receiver requests LL-HLS and reports its buffered/seekable ranges may the reserve shrink, with a four-second floor and a further 1.5-second margin beyond the observed native holdback. Slow-link RTT expands that reserve; unknown or conventional HLS receivers keep six seconds. The former two-second floor caused a startup deadlock against the three-segment readiness requirement and repeated stalls on Safari with a three-second seekable holdback; that floor has been removed. The UI bufferMs setting controls PCM transport credits, not Safari’s native seekable holdback. This reduces effect-change latency without dropping program samples, bypassing the shared playout barrier or sacrificing the native playback path. It is not zero-buffer instant effect switching. Diagnostics expose lowLatency and renderBufferMs so device negotiation can be verified instead of assumed. The room wait estimate now uses the generated-media lead from receiver diagnostics when available.

Reference: https://developer.apple.com/documentation/http-live-streaming/enabling-low-latency-http-live-streaming-hls . Automated checks cover fragment identity/timestamps, reset isolation, authenticated part access, query bounds, reserve negotiation and existing synchronization. Actual LL-HLS selection and sustained lock-screen playback on the user’s iPhone/iPad still require device observation.

Startup calibration now warms the actual native audio element silently and measures a second warm start before declaring synchronization readiness. Each probe returns to the held program position; mute state is restored on success, rejection, cancellation and track replacement. The host applies that measured lead to the first audible scheduled start rather than relying only on an audible drift correction several seconds into the song. Native HLS/background playback remains in use. Automated tests cover probe lifecycle and first-start scheduling; phone timing still needs device observation.
