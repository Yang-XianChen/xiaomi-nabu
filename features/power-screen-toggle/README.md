# Power / Lid Switch → Screen Toggle

Changes made on this machine (Ubuntu 26.04, GNOME Wayland):

1. `/etc/systemd/logind.conf.d/10-screen-toggle.conf`
   - Power key and lid switch are ignored by systemd-logind, so they no longer
     trigger poweroff / suspend.
2. `/usr/local/sbin/screen-toggle-daemon` + `screen-toggle.service`
   - A small daemon watches the power key (`pm8941_pwrkey`) and the lid switch
     (`gpio-keys` / SW_LID) and drives the panel backlight
     (`ktz8866-backlight`) through sysfs:
     * short power-key press → toggle screen off/on; turning it off starts a
       15 s auto-suspend countdown
   * lid close → screen off + 30 min auto-suspend countdown
   * lid open → screen on and cancels the countdown
   * any key press while the screen is dark → screen on (keyboard wake) and
     cancels the countdown
   * when a countdown expires without the system suspending, the daemon runs
     `systemctl suspend`; after wake it resets the countdown state and turns
     the screen back on
   * while charging, suspend control is disabled: the power key and lid only
     turn the screen off, and no auto-suspend/re-suspend runs
   Turning the screen off does two things: it asks the GNOME compositor to
   blank the display (`org.gnome.Mutter.DisplayConfig.PowerSaveMode = 1`,
   which disables the DRM connector/DPMS so the panel stops showing content
   entirely — no ghost image remains when the backlight is off) and then sets
   the backlight to 0. Turning it back on restores the display output and the
   previous brightness. Turning it back on also re-adds the pointer/touch
   touchscreen/pen devices (`udevadm trigger` remove+add), which makes mutter
   recompute the touchscreen-to-display mapping. Without this, the touch
   mapping can stay stale (e.g. portrait axes on a landscape screen) until
   the device is manually toggled. To keep screen-on fast, the refresh now
   runs in a background worker after the backlight is on: touchscreen/pen
   first, then mice/touchpads, so it never holds the backlight off.
   External wakes (for example GNOME idle blank dismissed by the keyboard) are detected by the DPMS poll loop and also trigger the input-device refresh. Brightness is restored by writing the user-set value (read before the backlight is dimmed, falling back to the state file when Mutter's property is stuck at the minimum) directly to sysfs, with a verification retry. If the Novatek display/touch
   controller reports a final init failure during wake (kernel logs such as
   `FW info is broken` or `download firmware failed`), the daemon
   automatically performs one extra blank/unblank cycle while the backlight
   stays at 0 before restoring brightness — this is the software version of
   the official “press power twice” workaround for persistent graphical
   glitches on nabu.
3. GNOME power settings (`org.gnome.settings-daemon.plugins.power`) for the
   `yangxc` user are set to `nothing` for the power button and both lid-close
   actions, so the desktop does not suspend or show a power dialog either.

Note: a hardware long-press on the power key (the PMIC-level force reset, if
supported by the device) cannot be overridden in software.
4. `/etc/systemd/system/screen-toggle-resume.service` — a oneshot unit that
   runs after the system wakes from suspend/hibernate and sends SIGUSR1 to the
   daemon, which restores the backlight and resyncs its on/off state, so the
   screen comes back on after wake.

   Note: the magnetic-contact Xiaomi keyboard is powered off by the hardware
   during suspend; no software re-enumeration (usbhid unbind/bind or the
   device-level `authorized` switch) can restore its power, so it must be
   physically re-attached after wake.

## Rust implementation

The daemon has been rewritten in Rust (source in `screen-toggle-rs/`, zero
third-party dependencies: std only plus a few raw libc FFI declarations).
Build and install:

```bash
cd screen-toggle-rs
cargo build --release
install -m 755 target/release/screen-toggle-daemon /usr/local/sbin/screen-toggle-daemon
systemctl restart screen-toggle.service
```

All external commands (gdbus / loginctl / udevadm / journalctl) run with a
hard timeout — D-Bus calls wait at most 5 seconds, then the whole process
group is killed and the call is treated as failed. This prevents a wedged
Mutter D-Bus call from blocking the daemon (and the power/lid keys) forever.

The screen-on path retries the compositor unblank up to 3 times and verifies
the real DPMS state before marking the screen as on; if the display cannot be
unblanked the screen stays marked off so the next lid/power event retries.
While the screen is on, the daemon tracks the sysfs brightness as the user's
real setting and ignores GNOME's idle-dim value (computed from the
`idle-brightness` percent over the panel's min..max range), so the restored
brightness follows the user's latest choice instead of a stale fixed value or
the auto-dimmed level.
Brightness restore goes through Mutter's `SetBacklight` D-Bus interface so the
GNOME brightness slider stays in sync with the real value; only the screen-off
write (0, which Mutter rejects) and D-Bus failures fall back to raw sysfs.
When the Novatek display/touch controller reports an init failure, up to 2
recovery blank/unblank cycles are attempted (3 unblank tries in total) before
the daemon gives up and restores the brightness anyway. Additionally, the
first screen-on after a system resume performs one unconditional
blank/unblank recovery cycle *instead of* a regular unblank (with the
backlight kept at 0): on this panel the firmware can log "Update firmware
success" while the screen stays black, so a full off/on cycle is required to
reliably light it up. Skipping the redundant first unblank keeps the extra
wake latency to roughly one blank plus a 300 ms pause.

The resume hook (`screen-toggle-resume.service`) signals only the daemon's
main process (`systemctl kill --kill-whom=main -s SIGUSR1 ...`) so it never
interrupts concurrent loginctl/runuser/gdbus subprocesses during a wake.

Keyboard, mouse and touchpad input wake the screen when it is off. Input
devices are re-scanned every few seconds so peripherals that connect later
(e.g. Bluetooth keyboards/mice) are picked up automatically, and input is
ignored while the lid is closed. Power-key presses queued during the slow
panel-recovery wake are detected by their event timestamp and ignored, so a
wake no longer toggles the screen back off right after it turns on.
