# OpenLess deployment notes (Xiaomi Pad 5 / Ubuntu arm64)

[English](README.md) | [简体中文](README.zh-CN.md)

OpenLess 1.3.16 (arm64) is a local voice-dictation tool: it transcribes
speech and types the text at the current cursor position. On this pad it is
paired with fcitx5, which provides the dictation hotkey and commit interface.

Tested environment: Xiaomi Pad 5 (nabu), Ubuntu 26.04 (resolute), GNOME
Wayland + Xwayland, fcitx5 5.1.19.

## Install

```bash
# 1. Install OpenLess itself (from the downloaded deb)
sudo apt install ./OpenLess_1.3.16_arm64.deb

# 2. Install libxdo3 — required at runtime but missing from the deb's Depends
sudo apt install libxdo3
```

Installed files:

- `/usr/bin/openless` — main program
- `/usr/share/applications/OpenLess.desktop` — app-menu entry
- `/usr/lib/aarch64-linux-gnu/fcitx5/libopenless.so` and
  `/usr/lib/OpenLess/linux-fcitx5-plugin/libopenless.so` — fcitx5 plugin
- `/usr/share/fcitx5/addon/openless.conf` — fcitx5 addon config

> The deb only declares `fcitx5`, `fcitx5-module-dbus`,
> `libwebkit2gtk-4.1-0` and `libgtk-3-0`, but the main binary directly links
> `libxdo.so.3`. Without `libxdo3` the program fails with:
>
> ```text
> openless: error while loading shared libraries:
> libxdo.so.3: cannot open shared object file: No such file or directory
> ```

## Make fcitx5 load the OpenLess plugin

If fcitx5 was already running before OpenLess was installed (the common
auto-start case), restart it once so the new addon is scanned:

```bash
fcitx5 -r -d
```

Verify the plugin is loaded:

```bash
# fcitx5 logs should show "OpenLess plugin loaded"
journalctl --user | grep -i openless | tail

# the DBus object should exist
gdbus introspect --session --dest org.fcitx.Fcitx5 --object-path /openless
```

The plugin registers `org.fcitx.Fcitx.OpenLess1`, exposing `CommitText`,
`SetHotkey`, `SetHotkeyRaw`, etc.

## Run and verify

```bash
openless
# or launch OpenLess from the app menu
```

A healthy startup log (`~/.local/share/OpenLess/logs/openless.log`) contains:

```text
[INFO] === OpenLess 启动 ===
[INFO] [fcitx-hotkey] fcitx5 available, syncing initial bindings (attempt 0)
[INFO] [fcitx] Synced hotkey Alt_R (sym=65514) to plugin via SetHotkeyRaw
[INFO] [fcitx] Resynced custom dictation trigger 'rightoption'
[INFO] [fcitx-hotkey] Listening for OpenLess1 signals
```

Check for missing libraries:

```bash
ldd /usr/bin/openless | grep -i 'not found' || echo "no missing libraries"
```

## Known non-blocking notices

- The startup warning
  `fcitx5 plugin .so not found in ["/usr/lib/x86_64-linux-gnu/fcitx5", ...]`
  is an arm64 path-detection bug in OpenLess: it only checks x86_64
  directories and never `/usr/lib/aarch64-linux-gnu/fcitx5`. The plugin is
  actually loaded by fcitx5 and DBus hotkey sync works, so this is harmless.
- `update endpoint did not respond with a successful status code` is only a
  failed online update check and does not affect local dictation.
- `libayatana-appindicator` deprecation and JACK/ALSA device messages come
  from other components and do not affect OpenLess.
