# charge-control：基于电量的充电控制

`features/charge-control/` 是配套的日常功能：守护进程按电池电量自动控制
LIONSEMI LN8000 充电芯片，避免长时间满电充电。

- 电量 **> 75%** 时停止充电；
- 电量 **< 50%** 时恢复充电；
- 电量在 50%~75% 之间时保持当前状态，避免在阈值边缘反复切换。

## 原理

该平板的内核驱动没有把充电开关暴露为 sysfs 接口，但驱动源码明确说明其工作
模式由 `SYS_CTRL`（寄存器 `0x1E`）控制：

- 置位 `STANDBY_EN`（bit 3）、清除 `EN_1TO1`（bit 0）→ standby，停止充电；
- 同时清除两个位 → switching，恢复充电。

程序通过 `/dev/i2c-1` 读写地址 `0x51` 的这颗芯片，只修改上述两个位，其余
寄存器位保持不变；启动时校验芯片 ID（应为 `0x42`）。每次决策都以硬件寄存器
实际状态重新断言，并每 2 秒综合 `ln8000-charger/present` 与 USB/Mains 的
`online` 判断电源是否接入（LN8000 驱动偶尔会漏记 present），因此拔插电源后
约 2 秒内会重新按电量阈值接管。

## 文件

- `charge-control-rs/`：Rust 实现（仅依赖 libc crate）
- `charge-control.service`：systemd 服务模板
- `install.sh`：编译并安装到 `/usr/local/sbin`，启用服务
- `配置文档.md`：阈值与参数配置说明
- `docs/技术文档.md`：完整技术实现方案

## 安装

```bash
sudo bash features/charge-control/install.sh
```

等价手动安装：

```bash
cd features/charge-control/charge-control-rs
cargo build --release
sudo install -m 755 target/release/charge-control /usr/local/sbin/charge-control
sudo install -m 644 ../charge-control.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now charge-control
```

## 只读查看状态

```bash
sudo /usr/local/sbin/charge-control --status
```

显示电量、充电器接入情况、芯片 ID、当前充电模式（switching / standby）等。

## 试运行（不写寄存器）

```bash
sudo /usr/local/sbin/charge-control --once --dry-run
```

## 自定义阈值

```bash
sudo /usr/local/sbin/charge-control --stop 80 --start 60 --interval 15
```

完整参数见 [配置文档.md](配置文档.md)。
