# Windows AirPods 头部追踪安装

SDA 的 Windows 头部追踪依赖一个实验性的、测试签名的 Bluetooth profile 驱动。SDA 安装向导会显示两个默认不勾选的独立选项：

- **Enable Windows TestSigning**：修改 Windows 启动配置，允许测试签名驱动；需要管理员权限和重启。
- **Install the SDA AirPods head-tracking driver**：校验内置证书 SHA-256，导入本地计算机的 `Root` 与 `TrustedPublisher` 证书库，再用 `pnputil` 安装驱动；需要管理员权限。

只有勾选选项后，安装器才会启动管理员 PowerShell。安装器不会自动重启 Windows。首次安装建议同时勾选两项，完成后手动重启。

## 安全说明

TestSigning 会降低 Windows 对内核驱动签名的强制要求，并在桌面显示测试模式水印。SDA 的 INF 使用 Apple Bluetooth VID 和明确的产品 PID 白名单，只匹配已确认支持 motion 数据的 AirPods Pro、Pro 2、AirPods 3、AirPods 4 和 AirPods Max 型号；AirPods 1/2 及未知型号不会匹配。

驱动证书 SHA-256 固定为：

```text
887FBB9BFF2D202DA0E0D828FEF7C0CA8B422193424F8C658E6ADB50A37EBFB5
```

安装日志位于：

```text
C:\ProgramData\SDA\Logs\head-tracking-driver-install.log
```

Secure Boot 策略可能拒绝开启 TestSigning。遇到这种情况，安装器会保留 SDA 主程序并显示失败提示；不要为了头追盲目关闭 Secure Boot。

## 关闭 TestSigning

不再使用测试驱动时，在管理员 PowerShell 中执行：

```powershell
bcdedit /set testsigning off
```

然后重启 Windows。关闭 TestSigning 不会自动删除已暂存的驱动包；如需卸载，先用 `pnputil /enum-drivers` 找到 `SdaAirPodsL2cap.inf` 对应的 Published Name，再以管理员身份执行：

```powershell
pnputil /delete-driver <Published Name> /uninstall
```

不要把示例中的 Published Name 写死成某个 `oemNN.inf`，该编号在每台电脑上都不同。
