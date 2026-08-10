# xiaomi-nabu — 内核 6.14.11（音频修复）

[English](README.md) | [简体中文](README.zh-CN.md)

适用于小米平板 5（nabu）的 Linux 内核与模块。本版本基于稳定的 **6.14.11** 内核，并包含音频修复。

## 特色适配功能

本 fork 附带两个日常适配功能，均包含在本仓库中：

### 电源 / 合盖息屏切换

`features/power-screen-toggle/` 是面向平板的日常配套功能：
一个小型守护进程，把电源键和合盖开关变成屏幕开关——短按电源键切换屏幕，
合盖关屏、开盖亮屏，并支持键盘/鼠标/触摸板唤醒、挂起倒计时、充电状态覆盖
以及面板自动恢复循环。

- 文档与源码：[features/power-screen-toggle/README.zh-CN.md](features/power-screen-toggle/README.zh-CN.md)
- 在设备上安装：`sudo bash features/power-screen-toggle/install.sh`

### 充电阈值控制

`features/charge-control/` 是面向平板的日常配套功能：
一个小型 Rust 守护进程，电量高于 75% 时停止充电、低于 50% 时恢复充电，
避免 LN8000 充电芯片和电池长时间处于满电状态。

- 文档与源码：[features/charge-control/README.zh-CN.md](features/charge-control/README.zh-CN.md)
- 在设备上安装：`sudo bash features/charge-control/install.sh`

## 最新发布

- **音频修复内核（推荐测试）：** `6.14.11-xiaomi-nabu-tmm-audio-fixes`
  - 独立启动项：`6.14.11-audio-fixes`
  - 安装包：`xiaomi-nabu-linux-6.14-audio-fixes_6.14.11-xiaomi-nabu-tmm-audio-fixes_arm64.deb`
- 下载：<https://github.com/Yang-XianChen/xiaomi-nabu/releases/latest>

## 安装音频修复内核

在你的小米平板 5 Ubuntu 系统上：

```bash
# 1. 下载安装包
curl -L -o xiaomi-nabu-linux-6.14-audio-fixes.deb \
  https://github.com/Yang-XianChen/xiaomi-nabu/releases/latest/download/xiaomi-nabu-linux-6.14-audio-fixes_6.14.11-xiaomi-nabu-tmm-audio-fixes_arm64.deb

# 2. 安装
sudo apt install ./xiaomi-nabu-linux-6.14-audio-fixes.deb
# 或：sudo dpkg -i xiaomi-nabu-linux-6.14-audio-fixes.deb

# 3. 重启并在启动菜单中选择 "6.14.11-audio-fixes"
sudo reboot
```

重启后验证当前内核：

```bash
uname -r
# 预期输出：6.14.11-xiaomi-nabu-tmm-audio-fixes
```

### 安装过程中会发生什么

音频修复包使用不同的包名和版本（`xiaomi-nabu-linux-6.14-audio-fixes`），
因此**不会覆盖**原来的 `xiaomi-nabu-linux-6.14` 包。

该包的 `postinst` 脚本：

1. 生成新的 UKI EFI 文件（`6.14.11-audio-fixes.efi`）。
2. 挂载 ESP 分区并复制到 `EFI/ubuntu/`。
3. **不会**删除或修改现有启动文件。

旧的 `6.14.11` 和 `6.17` 启动项保持不变；新内核会以额外启动项的形式出现。
如果 ESP 无法挂载，脚本会回退为把 `.efi` 文件复制到 `/boot`。

### 回滚 / 卸载

请保留原始内核包。如果音频修复内核无法启动，从引导管理器选择旧的 `6.14.11`
启动项，然后卸载新包：

```bash
sudo apt remove xiaomi-nabu-linux-6.14-audio-fixes
```

该包的 `postrm` 只会删除 `6.14.11-audio-fixes.efi`；原始启动项不受影响。

## 声音修复与配置

本内核包含以下音频修复：

- SLIMbus QMI 服务等待重试 —— 修复 ADSP 启动缓慢时音频始终无声音的问题。
- q6adm / q6asm 流关闭修复 —— 防止 DSP 会话泄漏和重新打开失败。
- CS35L41 PUP/PDN 错误降级为警告。
- ASM 流主增益控制（`nabu_stream_gain_q13`），默认 **-6 dB**，避免 DSP 削波。

**自动配置：** `xiaomi-nabu-linux-6.14-audio-fixes` 包在安装时会自动应用以下设置：

- 向 `/etc/modprobe.d/nabu-audio.conf` 写入 `options q6asm nabu_stream_gain_q13=4106`
- 在 `/etc/pulse/daemon.conf` 中设置 `default-fragment-size-msec = 256`

卸载或清除该包时，两项设置都会恢复为之前的状态。

### 验证音频模块

```bash
lsmod | grep -E 'snd_soc_sm8150|q6asm|q6adm|cs35l41'
aplay -l
sudo dmesg | grep -Ei 'slim|q6|cs35l41' | tail -30
```

### 修复削波或失真

默认流增益为 `4106`（约 -6 dB）。临时调整当前会话的增益：

```bash
echo 4106 | sudo tee /sys/module/q6asm/parameters/nabu_stream_gain_q13
```

增益值（Q13 线性）：`8192` = 0 dB，`5800` ≈ -3 dB，`4106` ≈ -6 dB。

永久生效：

```bash
echo 'options q6asm nabu_stream_gain_q13=4106' | sudo tee /etc/modprobe.d/nabu-audio.conf
sudo update-initramfs -u
sudo reboot
```

如果仍有削波，可继续调低数值（例如 `3500` ≈ -7.4 dB），或使用带限幅器的
Easy Effects；这可以处理 DSP 路径中与电平相关的削波。

### 音频缓冲区配置

如果出现爆音、卡顿或欠载，请增大音频缓冲区。编辑 `/etc/pulse/daemon.conf`
（或 `~/.config/pulse/daemon.conf`）：

```ini
default-fragments = 4
default-fragment-size-msec = 256
```

可选值：`128` ms、`256` ms、`512` ms。建议使用 `256` ms 或 `512` ms 以保证稳定。

**注意：** 缓冲区越大音频越稳定，但延迟也会增加。

重启音频服务（或重启系统）：

```bash
pulseaudio -k
# 或在 PipeWire 系统上：
systemctl --user restart pipewire-pulse
```

### 完全没有声音时

```bash
sudo modprobe snd_soc_sm8150
sudo dmesg | tail -50
aplay -l
```

确保已安装 `xiaomi-nabu-alsa` 包以获取正确的 ALSA UCM 配置：

```bash
sudo apt install xiaomi-nabu-alsa xiaomi-nabu-firmware
```

## 仓库结构

- `patches/` — 构建工作流应用的内核补丁。
- `kernel-build-files/` — 额外内核配置和 Debian `postinst`/`postrm` 脚本。
- `features/power-screen-toggle/` — 电源/合盖息屏切换配套守护进程。
- `features/charge-control/` — 电池阈值充电守护进程（LN8000，Rust）。
- `.github/workflows/build-kernel.yml` — 构建内核 `.deb`/`.tar` 安装包。

## 旧版 README

之前的完整安装指南（分区、刷写 Ubuntu、引导加载程序设置）仍可在这里查看：
[English](README-legacy.md) · [简体中文](README-legacy.zh-CN.md)
