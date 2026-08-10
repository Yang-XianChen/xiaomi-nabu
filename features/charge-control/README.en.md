# charge-control: battery-level-based charging control

[English](README.en.md) | [简体中文](README.md)

`features/charge-control/` is a companion daily-use feature: a daemon that
controls the LIONSEMI LN8000 charging chip automatically based on battery
level, avoiding long periods of charging at full capacity.

- Stops charging when battery level **> 75%**;
- Resumes charging when battery level **< 50%**;
- Keeps the current state between 50% and 75% to avoid toggling repeatedly at
  the threshold edges.

## How it works

The tablet's kernel driver does not expose a charging switch as a sysfs
interface, but the driver source clearly states that its operating mode is
controlled by `SYS_CTRL` (register `0x1E`):

- Set `STANDBY_EN` (bit 3) and clear `EN_1TO1` (bit 0) → standby, charging
  stopped;
- Clear both bits → switching, charging resumed.

The program reads/writes the chip at address `0x51` through `/dev/i2c-1`,
modifying only those two bits and leaving all other register bits unchanged;
it verifies the chip ID (should be `0x42`) at startup. Every decision is
re-asserted against the real hardware register state, and every 2 seconds it
combines `ln8000-charger/present` with USB/Mains `online` to determine whether
power is connected (the LN8000 driver occasionally misses `present`), so after
plugging/unplugging power it re-takes control by the battery threshold within
about 2 seconds.

## Files

- `charge-control-rs/` — Rust implementation (depends only on the libc crate)
- `charge-control.service` — systemd service template
- `install.sh` — builds and installs to `/usr/local/sbin` and enables the service
- [配置文档.en.md](配置文档.en.md) — threshold and parameter configuration
- [docs/技术文档.en.md](docs/技术文档.en.md) — full technical implementation details

## Installation

```bash
sudo bash features/charge-control/install.sh
```

Equivalent manual installation:

```bash
cd features/charge-control/charge-control-rs
cargo build --release
sudo install -m 755 target/release/charge-control /usr/local/sbin/charge-control
sudo install -m 644 ../charge-control.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now charge-control
```

## Read-only status

```bash
sudo /usr/local/sbin/charge-control --status
```

Shows battery level, charger presence, chip ID and the current charging mode
(switching / standby), etc.

## Dry run (without writing registers)

```bash
sudo /usr/local/sbin/charge-control --once --dry-run
```

## Custom thresholds

```bash
sudo /usr/local/sbin/charge-control --stop 80 --start 60 --interval 15
```

Full parameter list: [配置文档.en.md](配置文档.en.md).
