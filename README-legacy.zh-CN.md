# 小米平板 5（nabu）上的 Ubuntu Linux —— 安装指南

[English](README-legacy.md) | [简体中文](README-legacy.zh-CN.md)

![Ubuntu Linux on the Xiaomi Pad 5 (nabu)](ubuntu_resolute.png)

### ⚠️ 免责声明

*   **开始安装前，请先完整阅读一遍本指南。**
*   本项目与 Ubuntu、Canonical、小米或任何其他软硬件厂商均无官方关联，
    也不受其支持。
*   对于变砖的设备、损坏的 SD 卡或数据丢失，作者不承担任何责任。
*   是否进行这些修改由你自己决定，风险自负。
*   **！！！备份所有数据！！！** —— Android 的 `userdata` 在此过程中很可能会丢失。

---

## 🛠 准备工作

* 你的平板必须已解锁引导加载程序（bootloader）！
* 确保你已将所有必要文件下载到电脑上（点击左列文件名下载）：

| 文件 | 说明 | 作者与链接 |
| :--- | :--- | :--- |
| [V4-MODDED-TWRP-LINUX.img](https://github.com/TheMojoMan/xiaomi-nabu/releases/download/ubuntu-26.04-v1.0.0/V4-MODDED-TWRP-LINUX.img) | 用于访问内部磁盘（以及更多功能） | [ArKT-7](https://github.com/ArKT-7/twrp_device_xiaomi_nabu/releases/tag/mod_linux) |
| [efi.tar](https://github.com/TheMojoMan/xiaomi-nabu/releases/download/ubuntu-26.04-v1.0.0/efi.tar) | 包含启动 Linux 所需的文件 | [Timofey](https://github.com/timoxa0) 与 TheMojoMan |
| [setup_ubuntu.sh](https://github.com/TheMojoMan/xiaomi-nabu/releases/download/ubuntu-26.04-v1.0.0/setup_ubuntu.sh) | 分区磁盘并复制启动文件 | TheMojoMan |
| [ubuntu-26.04-xiaomi-nabu.img.xz](https://github.com/TheMojoMan/xiaomi-nabu/releases/download/ubuntu-26.04-v1.0.0/ubuntu-26.04-xiaomi-nabu.img.xz) | Ubuntu 根文件系统（已压缩） | [Canonical](https://cdimage.ubuntu.com/ubuntu-base/releases/26.04/beta/) 与 TheMojoMan |
| [installer_bootmanager_NOSB.zip](https://github.com/TheMojoMan/xiaomi-nabu/releases/download/ubuntu-26.04-v1.0.0/installer_bootmanager_NOSB.zip) | 修补引导加载程序以启动 LINUX（以及 ANDROID 等） | [rodriguezst](https://github.com/rodriguezst/nabu-dualboot-img) |

### 📦 重要：解压镜像

Ubuntu 镜像通常以压缩的 `.xz` 文件提供。刷写前**必须**先解压，才能得到真正的 `.img` 文件。

*   **Windows：** 使用免费工具 [7-Zip](https://www.7-zip.org/)。右键点击文件 -> *7-Zip* -> *解压到当前文件夹*。
*   **macOS：** 使用 [The Unarchiver](https://theunarchiver.com/)，或直接在 Finder 中双击文件。
*   **Linux（终端）：**
    ```bash
    xz -d ubuntu-26.04-xiaomi-nabu.img.xz
    ```

---

## 💻 新手教程：终端 / Shell 基础

如果你不熟悉命令行，请按以下步骤开始：

1.  **打开终端：**
    *   **Windows：** 按 `Win + R`，输入 `cmd`，回车。
    *   **macOS：** 按 `Cmd + Space`，输入 `Terminal`，回车。
    *   **Linux：** 你应该知道（取决于你使用的发行版）。

    **进入你的文件夹：** 输入 `cd `（<- 带空格），把包含下载文件的文件夹拖入终端，然后回车。
2.  **ADB/Fastboot：** 确保已安装 [Android Platform Tools](https://developer.android.com/studio/releases/platform-tools)。
3.  **历史记录：** 使用上下方向键翻看命令历史。使用 Ctrl+R 搜索命令历史。[Mac: Cmd+...]
4.  **自动补全：** 输入命令的前两个字母，然后按 TAB 键。按两次 TAB 可查看更多选项。
5.  **行编辑：** Ctrl+A/E：光标移动到行首/行尾；Ctrl+U/K：删除光标左侧/右侧文本。[Mac: Cmd+...]
6.  **复制/粘贴：** 用鼠标选中文本，然后右键选择复制（Shift+Ctrl+c）或粘贴（Shift+Ctrl+v）。[Mac: Shift+Cmd+...]

---

## 1. 准备与分区

1.  让平板进入 **Fastboot 模式**：关机后，用 USB 线连接平板与电脑/Mac，
    同时按住“音量下”键。平板应开机并在数秒后显示 'Fastboot'，然后松开“音量下”键。
2.  从电脑启动修改版 TWRP（如果第一次没成功，请重试）：
    ```bash
    fastboot boot V4-MODDED-TWRP-LINUX.img
    ```
3.  TWRP 加载后，把 setup 文件推送到平板，进入平板 shell 并运行工具：
    ```bash
    adb push setup_ubuntu.sh tmp
    adb push efi.tar tmp
    adb shell "chmod +x tmp/setup_ubuntu.sh && tmp/setup_ubuntu.sh"
    ```

    **脚本菜单内：**
    *   **试运行（Dry-Run）：** 先选择“Yes”安全模拟整个流程。
    *   **分区：**
        *   **单系统（Single-Boot）：** 选择**擦除** `userdata`（选项 1）。
            这会彻底删除 Android！！！
        *   **双系统（Dual-Boot）：** 选择**调整大小** `userdata`（选项 2）。
            这会保留 Android（但数据很可能丢失）。
    *   **布局：** 选择 `esp + linux + data`（选项 2）以获得最佳体验。
        这样以后安装新版本 Linux 时无需丢失所有数据。
    *   **完成：** 脚本会自动格式化 ESP，并从 `efi.tar` 安装引导加载程序文件。

---

## 2. 刷写 Ubuntu

分区完成后设备会回到 **Fastboot 模式**，此时刷写真正的操作系统：

```bash
# 刷写未压缩的 Ubuntu 根文件系统镜像
fastboot flash linux ubuntu-26.04-xiaomi-nabu.img
```

---

## 3. 修补 Android 引导加载程序

为了能够启动 Linux（以及磁盘上的其他系统），需要通过 ADB sideload 安装
**rodriguezst** 的启动镜像修补器：

1.  在平板上返回 **TWRP 主界面**。
2.  点击 **Advanced** -> 点击 **ADB Sideload** -> **滑动屏幕上的滑块**。
3.  在电脑上运行 adb sideload 命令：
    ```bash
    adb sideload installer_bootmanager_NOSB.zip
    ```
4.  完成后即可重启。你的设备现在可以启动 Linux（也可以切换到 Android 或其他已安装系统）。

---

## 🌐 备选：网页工具（Arkt-7）

如果你更喜欢图形界面，可以使用 Arkt-7 的网页工具
[arkt-7.github.io/nabu/](https://arkt-7.github.io/nabu/)。
*   它允许你通过 WebUSB 启动 TWRP 并刷写镜像。
*   你仍然需要按上文所述把 `setup_ubuntu.sh` 和 `efi.tar` 推送到 tmp 文件夹。
*   通过 TWRP 终端运行 `setup_ubuntu.sh`：点击 **Advanced** -> 点击 **Terminal** ->
    输入 `chmod +x tmp/setup_ubuntu.sh && tmp/setup_ubuntu.sh`。

---

## ⚠️ 故障排查与已知问题

*   **内核：** **内核 6.17 已弃用。** 请使用 **6.14.11**，它能在每台平板上启动。
*   **文件应用：** 长按文件夹会打开右键菜单，但长按背景不会；
    请用鼠标右键，或在终端中用 `mkdir folder_name` 创建新文件夹。
*   **挂起：** 挂起后屏幕有时无法再次点亮。

---

## 安装后

启动 Ubuntu 并完成初始设置后，连接互联网（如果之前没有连接）。

**更新与升级：**
*  打开终端 -> 点击左侧栏顶部的图标。
*  输入 `cat .bash_aliases`。这会显示我为你（和我自己）定义的快捷方式列表，
   可以省去大量输入！

   [你可以用 `micro .bash_aliases` 编辑该文件添加自己的快捷方式。
   修改后用 "Ctrl+s" 保存，用 "Ctrl+q" 退出。]
*  更新软件包列表：`sudo apt update`（快捷方式：`sau`）。
*  升级软件包（如果有可用更新）：`sudo apt upgrade`（快捷方式：`saug`）。

   说明：`sudo` 命令可获取超级用户（root）权限。只有超级用户（root）才能安装新软件。

**修改语言设置：**
*  点击右上角的电池图标。
*  点击设置图标。
*  滚动左侧栏，直到在最底部看到 'System'，点击它。
*  点击 'Region & Language' -> 'Manage Installed Languages' -> 'Install/Remove Languages...'。
*  勾选要安装的语言并点击 'Apply'。
*  把新语言拖到列表顶部。<- 已知问题：必须使用鼠标！
*  点击 'Apply System-Wide' 和 'Close'。为 'Your Account' 设置首选语言和格式。
*  注销并重新登录 -> 系统现在应该使用你选择的语言。

**安装一些常用应用：**
*  VLC：`sudo apt install firefox`（快捷方式：`sai vlc`）。
*  Blender：`sudo apt install blender`（快捷方式：`sai blender`）。
*  Firefox 浏览器：`sudo snap install firefox`（快捷方式：`ssi firefox`）。
*  Ubuntu 应用商店：`sudo snap install snap-store`（快捷方式：`ssi snap-store`）。
*  LocalSend：`sudo snap install localsend`（快捷方式：`ssi localsend`）。
*  Telegram：`sudo snap install telegram-desktop`（快捷方式：`ssi telegram-desktop`）。

**使用你的 'data' 分区：**
*  输入 `sudo micro /etc/fstab`。删除最后一行的 "#" 符号。用 "Ctrl+s" 保存，
   用 "Ctrl+q" 退出并**重启**。
*  重启后再次打开终端，输入 `sudo chown <你的用户名>:<你的用户名> /media/data`。
   例如用户名是 `tom`：`sudo chown tom:tom /media/data`。
*  可选：请使用搜索引擎或 AI 助手了解如何将主目录（下载/图片等）永久链接到 'data' 分区。

**使用小米手写笔：**
*  手写笔无需额外操作即可使用。

---

## 致谢

感谢以下人员（大致按时间顺序）为小米平板 5 开发 Linux 内核和/或关键工具所做的出色工作。
没有他们，这一切都不可能实现：
*   **Alexandru Marc Serdeliuc** <https://github.com/serdeliuk>
*   **map220v** <https://github.com/map220v>
*   **maverickjb** <https://github.com/maverickjb>
*   **Pan Ortiz** <https://gitlab.com/panpanpanpan>
*   **Viola Guerrera** <https://github.com/nik012003>
*   **rodriguezst** <https://github.com/rodriguezst>
*   **Timofey** <https://github.com/timoxa0>
*   **Amrit Ranjan** <https://github.com/arkt-7>
*   **jhuang** <https://github.com/jhuang6451>
*   **gmanka** <https://github.com/gmankab>

---

## 其他优秀的 nabu Linux 发行版

* [postmarketOS](https://wiki.postmarketos.org/wiki/Xiaomi_Pad_5_%28xiaomi-nabu%29) —— nabu 的 pmOS
* [fedora_nabu](https://github.com/jhuang6451/nabu_fedora) —— nabu 的 Fedora
* [pocketblue](https://github.com/pocketblue/pocketblue) —— 面向移动设备的 Fedora Atomic

---

## 小米平板 5 Telegram 群组

<https://t.me/nabulinux> —— 特别感谢 Mateus Lima 管理该群组！
