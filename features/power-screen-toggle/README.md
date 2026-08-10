# Power / Lid Switch → Screen Toggle

A small companion daemon that turns the power button and lid switch on a
Linux tablet / laptop into display controls: short-press power toggles the
screen, lid close turns it off, lid open turns it on, and keyboard / mouse /
touchpad input can wake it. Designed and tested on the Xiaomi Pad 5 (nabu)
running Ubuntu with GNOME Wayland.

## Features

- Short power-key press toggles the screen off/on; turning it off starts a
  15 s auto-suspend countdown.
- Lid close → screen off + 30 min auto-suspend countdown; lid open → screen
  on and cancels the countdown.
- Any key press, mouse movement/click or touchpad gesture wakes the screen
  while it is dark.
- While charging, suspend control is disabled: the power key and lid only
  turn the screen off.
- The display is blanked through the GNOME compositor
  (`org.gnome.Mutter.DisplayConfig.PowerSaveMode`) and the backlight is set
  to 0, so no content or ghost image remains.
- The previous user brightness is restored on wake; GNOME's auto-dim value is
  recognized and ignored, and the GNOME brightness slider stays in sync with
  the real value (brightness is restored through Mutter's `SetBacklight`).
- Automatic recovery for the Novatek display/touch controller: if the panel
  reports an init failure during wake, the daemon performs extra blank/unblank
  cycles (backlight kept at 0) equivalent to the "press power twice"
  workaround.
- Input devices are re-scanned periodically, so keyboards/mice that connect
  later (e.g. Bluetooth) are picked up automatically; lid-closed input is
  ignored.

## Installation

Run from this directory (as root):

```bash
sudo bash install.sh
```

The script:

1. Builds the Rust daemon (`screen-toggle-rs/`).
2. Installs `/usr/local/sbin/screen-toggle-daemon`.
3. Installs `screen-toggle.service` and `screen-toggle-resume.service`.
4. Installs the logind override `10-screen-toggle.conf` (power key / lid
   switch are ignored by systemd-logind).
5. Enables and starts the service, and best-effort sets the GNOME power
   button / lid actions to "nothing" for the invoking user.

A reboot is recommended so systemd-logind picks up the new override.

Manual build/install:

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

## GNOME power settings

The GNOME settings-daemon should be configured so the desktop does not
suspend or show a power dialog on power-button / lid events (the daemon
handles them):

```bash
gsettings set org.gnome.settings-daemon.plugins.power power-button-action nothing
gsettings set org.gnome.settings-daemon.plugins.power lid-close-ac-action nothing
gsettings set org.gnome.settings-daemon.plugins.power lid-close-battery-action nothing
```

## Behavior details

- **Input detection:** the daemon auto-detects the power key, lid switch,
  keyboards (`ID_INPUT_KEYBOARD`) and pointer devices
  (`ID_INPUT_MOUSE` / `ID_INPUT_TOUCHPAD` / `ID_INPUT_POINTINGSTICK`) from
  udev; it does not depend on fixed event numbers. New devices are picked up
  within a few seconds.
- **Command timeouts:** all external commands (gdbus / loginctl / udevadm /
  journalctl) run with a hard timeout, so a wedged D-Bus call can never block
  the daemon forever.
- **Screen-on reliability:** the compositor unblank is retried and verified
  against the real DPMS state; if it fails, the screen stays marked off and
  the next event retries.
- **Panel recovery:** after a system resume the first screen-on runs one
  full blank/unblank cycle (backlight kept at 0); if the kernel logs an
  explicit init failure, up to two additional recovery cycles are attempted.
- **Wake timing:** after the daemon turns the screen off, pointer wake is
  ignored for 2 seconds (power key and keyboard still work) and the power-key
  debounce is reset, so pressing power again immediately wakes the screen even
  while the mouse is still being moved.
- **Stale key presses:** power-key presses queued while a slow recovery wake
  is still running are detected by their event timestamp and ignored, so a
  wake does not toggle the screen back off right after it turns on.

## Files

- `screen-toggle-rs/` — Rust daemon source (std only + a little libc FFI).
- `screen-toggle.service` / `screen-toggle-resume.service` — systemd units.
- `10-screen-toggle.conf` — systemd-logind override.
- `install.sh` — installation script.
- `配置文档.md` — detailed configuration notes (Chinese).

## Uninstall

```bash
sudo systemctl disable --now screen-toggle.service screen-toggle-resume.service
sudo rm -f /etc/systemd/system/screen-toggle.service /etc/systemd/system/screen-toggle-resume.service
sudo rm -f /etc/systemd/logind.conf.d/10-screen-toggle.conf
sudo rm -f /usr/local/sbin/screen-toggle-daemon
sudo rm -rf /var/lib/screen-toggle
sudo systemctl daemon-reload
# restore GNOME defaults if desired:
gsettings reset org.gnome.settings-daemon.plugins.power power-button-action
gsettings reset org.gnome.settings-daemon.plugins.power lid-close-ac-action
gsettings reset org.gnome.settings-daemon.plugins.power lid-close-battery-action
```

## Notes / limitations

- A hardware long-press on the power key (PMIC-level force reset, if
  supported) cannot be overridden in software.
- A magnetic-contact keyboard may be powered off by the hardware during
  suspend and must be re-attached after wake.
- Touchscreen / pen are not wake sources by design (to avoid accidental
  wakes), but they are refreshed after screen-on for correct touch mapping.
