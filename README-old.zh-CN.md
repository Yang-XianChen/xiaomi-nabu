# xiaomi-nabu

[English](README-old.md) | [简体中文](README-old.zh-CN.md)

适用于小米平板 5（代号：nabu）的 Linux 磁盘镜像、内核与脚本。

# 新闻

 - 2026.04.08：预计 Ubuntu 26.04 将在未来几天内发布……（仍在测试中，安装说明需要大改）
 - 2025.08.24：镜像已更新到内核 6.14.11（6.16 在部分设备上尚无法启动）：
   - USB 音频和 MIDI 现在开箱即用
   - 使用 `sudo qbootctl -s a` 或 `sudo qbootctl -s b` 切换插槽
 - 我已停止支持 Fedora。如果你需要 nabu 的 Fedora，请前往：https://github.com/nik012003/nabu-fedora-builder
 - ~2025.04.21：我已为 nabu 上传了 "Fedora 42"！~
   ~安装说明（见下文）同样适用，即在 mega.nz 页面上的 "2025.04.21-Fedora42" 文件夹中找到文件，
   根镜像名为 "fedora-42-nabu.img.xz"，Fedora 的包管理器为 "dnf"（而非 "apt"）。~
   ~请继续阅读下面的说明！~

# 小米平板 5（nabu）上的 Ubuntu Linux

![Ubuntu Linux on the Xiaomi Pad 5 (nabu)](ubuntu-nabu.png)

## 感谢

首先，我要感谢以下人员（排名不分先后）为小米平板 5 开发 Linux 内核所做的出色工作。
没有他们，这一切都不可能实现：
 - Alexandru Marc Serdeliuc <https://github.com/serdeliuk>
 - map220v <https://github.com/map220v>
 - maverickjb <https://github.com/maverickjb>
 - Pan Ortiz <https://gitlab.com/panpanpanpan>
 - ……（稍后补充）

他们的工作建立在前人的肩膀上。因此，感谢 Linux 内核团队、Ubuntu 团队、Gnome 和 KDE 团队，
以及所有为这个发行版所使用的程序做出贡献的人们！

另外还要感谢 "Xiaomi Pad 5 - Linux" Telegram 群组的各位热心成员：t.me/nabulinux

## 安装 Ubuntu 25.04（Plucky Puffin）

 - **！！！先备份 Android 中的数据！！！** 你很可能会失去 Android 并需要重新安装。继续操作风险自负！
 - 用 USB-C 线连接平板和电脑。
 - 打开终端窗口。
 - 安装 adb 和 fastboot：`sudo apt install adb android-sdk-platform-tools`。
   在 Windows 或 Mac 上：[在此下载](https://developer.android.com/tools/releases/platform-tools)。
 - 首先需要缩小小米平板 5 内置存储的 "userdata" 分区，并创建两个新分区，分别命名为
   "esp" 和 "linux"（不带引号）。有多种方法可以做到，例如参考 [这篇指南](https://xdaforums.com/t/resize-internal-storage-on-xiaomi-pad-5-nabu-and-install-another-images.4642670/)。
   "esp" 应为 vfat 格式、1 GB 大小，"linux" 应为 ext4 格式、至少 20-30 GB。
 - 接下来，下载 Ubuntu 25.04 根镜像文件和引导加载程序。你可以在 [这里](https://mega.nz/folder/CVMGEAiB#7oazR3wpkKdAH2eZChtRTg)
   的 "Ubuntu 25.04 (Plucky Puffin)" 文件夹中找到它们。
 - 解压 ubuntu-25.04.img.xz：`xz -d ubuntu-25.04.img.xz`。在 Windows 上：安装合适的解压工具。
 - 进入 fastboot（`adb reboot bootloader`，或关机后按住音量下键开机）。
 - 检查当前插槽：`fastboot getvar current-slot`
   如果显示 "a"，则需要把 Linux 引导加载程序安装到 boot_b；如果显示 "b"，
   则安装到 boot_a。
 - 删除 dtbo：`fastboot erase dtbo_b`（如果在 b 插槽，则执行 `fastboot erase dtbo_a`）。
 - 安装 Ubuntu 根系统：`fastboot flash linux ubuntu-25.04.img`
 - 安装引导加载程序：`fastboot flash boot_b boot_6.14.11-nabu-tmm_linux.img`
   （如果在 b 插槽，则执行 `fastboot flash boot_a boot_6.14.11-nabu-tmm_linux.img`）
 - 切换到 Ubuntu 插槽：`fastboot set_active b`（如果在 b 插槽，则执行 `fastboot set_active a`）
 - 重启：`fastboot reboot`
 - 等待 Ubuntu 完全启动。输出可能在某个位置卡住几秒钟，不要慌张，请耐心等待。
 - 完成初始设置并创建你的用户账户。
 - （更优雅的 Android/Linux 切换启动方法将在稍后介绍！）

## 安装后（在你的小米平板 5 上）

（如果尚未连接互联网，请先连接。）

打开终端：
 - 提示：在 Linux 中，输入命令或文件路径的前几个字母后按 **tab** 键会自动补全。
 - 输入 `cat .bash_aliases`。这会显示我为你（和我自己）定义的快捷方式列表，
   可以省去大量输入！

   （你可以用 `nano .bash_aliases` 添加自己的快捷方式。修改后用 "Ctrl+s" 保存，
   用 "Ctrl+q" 退出。）
 - 更新软件包列表：`sudo apt update`（快捷方式：`sau`）。
 - 升级软件包（如果有可用更新）：`sudo apt upgrade`（快捷方式：`saug`）。
 - 提示：`sudo` 命令可获取超级用户（root）权限。只有超级用户（root）才能安装新软件。

你可能会想安装一些常用应用：
 - 安装 Firefox 浏览器：`sudo apt install firefox`（快捷方式：`sai firefox`）。
 - 安装 Ubuntu 应用商店：`sudo snap install snap-store`（快捷方式：`ssi snap-store`）。

## 更新

内核更新会发布在这里，因为它们不在 Ubuntu 官方软件源中。
你可以加入专门的 Telegram 群组获取最新消息：t.me/nabulinux

如果你对 Linux 和 Ubuntu 有更多问题，请使用搜索引擎，网上有很多优秀的论坛和网站可以帮你。

## 最后

最重要的是：😀 **好好享受你的新超便携电脑！** 😀
