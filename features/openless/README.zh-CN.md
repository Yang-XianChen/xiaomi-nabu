# OpenLess 部署记录（Xiaomi Pad 5 / Ubuntu arm64）

OpenLess 1.3.16（arm64）是本地语音听写工具：语音转文字后直接输入到当前
光标位置，配合 fcitx5 插件提供听写热键和提交接口。本机为 Xiaomi Pad 5
（nabu），Ubuntu 26.04（resolute），GNOME Wayland + Xwayland，fcitx5
5.1.19。

## 安装

```bash
# 1. 安装 OpenLess 本体（下载的 deb 放在当前目录）
sudo apt install ./OpenLess_1.3.16_arm64.deb

# 2. 补装运行时依赖 libxdo3（deb 的 Depends 字段漏掉了它）
sudo apt install libxdo3
```

安装内容：

- `/usr/bin/openless` — 主程序
- `/usr/share/applications/OpenLess.desktop` — 应用菜单入口
- `/usr/lib/aarch64-linux-gnu/fcitx5/libopenless.so` 与
  `/usr/lib/OpenLess/linux-fcitx5-plugin/libopenless.so` — fcitx5 插件
- `/usr/share/fcitx5/addon/openless.conf` — fcitx5 插件配置

> **注意**：OpenLess 1.3.16 的 deb 依赖声明只有 `fcitx5`、
> `fcitx5-module-dbus`、`libwebkit2gtk-4.1-0`、`libgtk-3-0` 等，没有声明
> `libxdo3`，但主程序直接链接 `libxdo.so.3`。只装 deb 会报：
>
> ```text
> openless: error while loading shared libraries:
> libxdo.so.3: cannot open shared object file: No such file or directory
> ```
>
> 必须额外安装 `libxdo3`。

## 让 fcitx5 加载 OpenLess 插件

如果 fcitx5 在 OpenLess 安装之前就已经在运行（开机自启的常见情况），需要
重启一次 fcitx5 才会扫描并加载新插件：

```bash
fcitx5 -r -d
```

重启后确认插件加载：

```bash
# fcitx5 日志应出现 OpenLess plugin loaded
journalctl --user | grep -i openless | tail

# DBus 对象应存在
gdbus introspect --session --dest org.fcitx.Fcitx5 --object-path /openless
```

插件注册的接口为 `org.fcitx.Fcitx.OpenLess1`，提供 `CommitText`、
`SetHotkey`、`SetHotkeyRaw` 等听写提交/热键方法。

## 启动与验证

```bash
openless
# 或从应用菜单启动 OpenLess
```

正常启动后日志（`~/.local/share/OpenLess/logs/openless.log`）应出现：

```text
[INFO] === OpenLess 启动 ===
[INFO] [fcitx-hotkey] fcitx5 available, syncing initial bindings (attempt 0)
[INFO] [fcitx] Synced hotkey Alt_R (sym=65514) to plugin via SetHotkeyRaw
[INFO] [fcitx] Resynced custom dictation trigger 'rightoption'
[INFO] [fcitx-hotkey] Listening for OpenLess1 signals
```

动态库完整性检查：

```bash
ldd /usr/bin/openless | grep -i 'not found' || echo "no missing libraries"
```

## 已知的非阻塞提示

- 启动日志中的
  `fcitx5 plugin .so not found in ["/usr/lib/x86_64-linux-gnu/fcitx5", ...]`
  是 OpenLess 对 arm64 路径适配不全导致的误报：它只检查 x86_64 目录，
  不会检查 `/usr/lib/aarch64-linux-gnu/fcitx5`。插件实际已被 fcitx5 加载，
  DBus 同步也正常，不影响使用。
- `update endpoint did not respond with a successful status code` 只是
  在线更新检查失败，不影响本地听写功能。
- `libayatana-appindicator` 弃用警告、JACK/ALSA 设备提示均来自其他组件，
  不影响 OpenLess 运行。
