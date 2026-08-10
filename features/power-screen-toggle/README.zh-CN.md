# 电源键 / 合盖开关 → 屏幕切换

[English](README.md) | [简体中文](README.zh-CN.md)

一个小型配套守护进程，把 Linux 平板/笔记本上的电源键和合盖开关变成显示控制：
短按电源键切换屏幕，合盖关屏，开盖亮屏，键盘/鼠标/触摸板输入可以唤醒屏幕。
已在运行 Ubuntu + GNOME Wayland 的小米平板 5（nabu）上设计和测试。

## 功能

- 短按电源键切换屏幕关闭/打开；关屏时会启动 15 秒自动挂起倒计时。
- 合盖 → 关屏并启动 30 分钟自动挂起倒计时；开盖 → 亮屏并取消倒计时。
- 屏幕变暗（关闭）期间，任意按键、鼠标移动/点击或触摸板手势都会唤醒屏幕。
- 充电状态下禁用挂起控制：电源键和合盖只会关屏。
- 通过 GNOME 合成器（`org.gnome.Mutter.DisplayConfig.PowerSaveMode`）关闭显示，
  同时把背光设为 0，因此不会残留任何内容或残影。
- 唤醒时恢复用户之前的亮度；能识别并忽略 GNOME 的自动调暗值，
  且 GNOME 亮度滑块与真实值保持同步（通过 Mutter 的 `SetBacklight` 恢复亮度）。
- 针对 Novatek 显示/触摸控制器的自动恢复：如果面板在唤醒期间报告初始化失败，
  守护进程会执行额外的 blank/unblank 循环（背光保持 0），等效于“按两次电源键”的临时方案。
- 输入设备会定期重新扫描，因此后连接的键盘/鼠标（例如蓝牙设备）会被自动纳入；
  合盖状态下忽略输入。

## 安装

在本目录下以 root 身份运行：

```bash
sudo bash install.sh
```

脚本会：

1. 编译 Rust 守护进程（`screen-toggle-rs/`）。
2. 安装 `/usr/local/sbin/screen-toggle-daemon`。
3. 安装 `screen-toggle.service` 和 `screen-toggle-resume.service`。
4. 安装 logind 覆盖文件 `10-screen-toggle.conf`（systemd-logind 忽略电源键/合盖开关）。
5. 启用并启动服务，并尽力为调用用户把 GNOME 的电源键/合盖动作设置为“无操作”。

建议重启，以便 systemd-logind 读取新的覆盖文件。

手动编译/安装：

```bash
cd screen-toggle-rs
cargo build --release
sudo install -m 755 target/release/screen-toggle-daemon /usr/local/sbin/screen-toggle-daemon
sudo install -m 644 ../screen-toggle.service /etc/systemd/system/
sudo install -m 644 ../screen-toggle-resume.service /etc/systemd/system/
sudo install -m 644 ../10-screen-toggle.conf /etc/systemd/logind.conf.d/
sudo systemctl daemon-reload
sudo systemctl enable --now screen-toggle.service
```

## GNOME 电源设置

应配置 GNOME settings-daemon，使桌面在电源键/合盖事件时不挂起或显示电源对话框
（这些事件由守护进程处理）：

```bash
gsettings set org.gnome.settings-daemon.plugins.power power-button-action nothing
gsettings set org.gnome.settings-daemon.plugins.power lid-close-ac-action nothing
gsettings set org.gnome.settings-daemon.plugins.power lid-close-battery-action nothing
```

## 行为细节

- **输入检测：** 守护进程通过 udev 自动检测电源键、合盖开关、键盘
  （`ID_INPUT_KEYBOARD`）和指针设备（`ID_INPUT_MOUSE` / `ID_INPUT_TOUCHPAD` /
  `ID_INPUT_POINTINGSTICK`），不依赖固定的事件编号。新设备会在几秒内被纳入监听。
- **命令超时：** 所有外部命令（gdbus / loginctl / udevadm / journalctl）
  都带有硬超时，因此卡住的 D-Bus 调用永远不会永久阻塞守护进程。
- **亮屏可靠性：** 合成器 unblank 会重试，并与真实 DPMS 状态核对；
  如果失败，屏幕仍保持“已关闭”标记，下一次事件会再次尝试。
- **外部唤醒后的亮度：** 当键盘/鼠标输入解除 GNOME 空闲自动息屏时，
  GNOME 可能把背光停留在自动调暗值（或 0）；守护进程会检测到这一点并恢复用户的真实亮度。
- **面板恢复：** 系统唤醒后的第一次亮屏会执行一次完整的 blank/unblank 循环
  （背光保持 0）；如果内核日志明确记录初始化失败，还会再尝试最多两次恢复循环。
- **唤醒时机：** 守护进程关屏后，指针唤醒会被忽略 2 秒（电源键和键盘不受影响），
  并重置电源键防抖，因此即使鼠标还在移动，再次按电源键也能立即唤醒屏幕。
- **过期按键：** 在缓慢的恢复唤醒过程中排队的电源键事件，会按其事件时间戳识别为
  “亮屏前按下”而被忽略，避免屏幕刚亮起又被立刻关闭。

## 文件

- `screen-toggle-rs/` — Rust 守护进程源码（仅标准库 + 少量 libc FFI）。
- `screen-toggle.service` / `screen-toggle-resume.service` — systemd 单元。
- `10-screen-toggle.conf` — systemd-logind 覆盖文件。
- `install.sh` — 安装脚本。
- [配置文档.md](配置文档.md) — 详细配置说明（中文）。

## 卸载

```bash
sudo systemctl disable --now screen-toggle.service screen-toggle-resume.service
sudo rm -f /etc/systemd/system/screen-toggle.service /etc/systemd/system/screen-toggle-resume.service
sudo rm -f /etc/systemd/logind.conf.d/10-screen-toggle.conf
sudo rm -f /usr/local/sbin/screen-toggle-daemon
sudo rm -rf /var/lib/screen-toggle
sudo systemctl daemon-reload
# 如需恢复 GNOME 默认设置：
gsettings reset org.gnome.settings-daemon.plugins.power power-button-action
gsettings reset org.gnome.settings-daemon.plugins.power lid-close-ac-action
gsettings reset org.gnome.settings-daemon.plugins.power lid-close-battery-action
```

## 注意事项 / 限制

- 硬件长按电源键（如果支持 PMIC 级强制复位）无法通过软件覆盖。
- 磁吸键盘在挂起期间可能被硬件断电，唤醒后需要重新吸附。
- 触摸屏/手写笔按设计不作为唤醒源（避免意外唤醒），但亮屏后会刷新，
  以保证触摸映射正确。
