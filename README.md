# xiaomi-nabu — Kernel 6.14.11 (audio fixes)

Linux kernel and modules for the Xiaomi Pad 5 (nabu). This release is based on the stable **6.14.11** kernel with audio fixes.

## Latest release

- **Audio-fixes kernel (recommended for testing):** `6.14.11-xiaomi-nabu-tmm-audio-fixes`
  - Independent boot entry: `6.14.11-audio-fixes`
  - Package: `xiaomi-nabu-linux-6.14-audio-fixes_6.14.11-xiaomi-nabu-tmm-audio-fixes_arm64.deb`
- Download: <https://github.com/Yang-XianChen/xiaomi-nabu/releases/latest>

## Apply the audio-fixes kernel

On your Xiaomi Pad 5 Ubuntu system:

```bash
# 1. Download the package
curl -L -o xiaomi-nabu-linux-6.14-audio-fixes.deb \
  https://github.com/Yang-XianChen/xiaomi-nabu/releases/latest/download/xiaomi-nabu-linux-6.14-audio-fixes_6.14.11-xiaomi-nabu-tmm-audio-fixes_arm64.deb

# 2. Install it
sudo apt install ./xiaomi-nabu-linux-6.14-audio-fixes.deb
# or: sudo dpkg -i xiaomi-nabu-linux-6.14-audio-fixes.deb

# 3. Reboot and select "6.14.11-audio-fixes" in the boot menu
sudo reboot
```

After reboot, verify the running kernel:

```bash
uname -r
# expected output: 6.14.11-xiaomi-nabu-tmm-audio-fixes
```

### What happens during installation

The audio-fixes package uses a different package name and version (`xiaomi-nabu-linux-6.14-audio-fixes`), so it **does not overwrite** the original `xiaomi-nabu-linux-6.14` package.

The package's `postinst` script:

1. Builds a new UKI EFI file (`6.14.11-audio-fixes.efi`).
2. Mounts the ESP partition and copies it to `EFI/ubuntu/`.
3. Does **not** delete or modify existing boot files.

Old `6.14.11` and `6.17` boot entries remain unchanged; the new kernel appears as an additional entry. If the ESP cannot be mounted, the script falls back to copying the `.efi` file to `/boot` instead.

### Rollback / removal

Keep the original kernel package installed. If the audio-fixes kernel does not boot, select the old `6.14.11` entry from your boot manager, then remove the new package:

```bash
sudo apt remove xiaomi-nabu-linux-6.14-audio-fixes
```

The package's `postrm` removes only `6.14.11-audio-fixes.efi`; the original entries are untouched.

## Sound fixes and configuration

This kernel includes these audio fixes:

- SLIMbus QMI service wait with retries — fixes audio staying dead after a slow ADSP boot.
- q6adm / q6asm stream close fixes — prevents DSP session leaks and failed re-opens.
- CS35L41 PUP/PDN errors demoted from errors to warnings.
- ASM stream master gain control (`nabu_stream_gain_q13`), default **-6 dB** to avoid DSP clipping.

**Automatic configuration:** the `xiaomi-nabu-linux-6.14-audio-fixes` package applies these settings automatically on install:

- Writes `options q6asm nabu_stream_gain_q13=4106` to `/etc/modprobe.d/nabu-audio.conf`
- Sets `default-fragment-size-msec = 256` in `/etc/pulse/daemon.conf`

Both are restored to their previous state when the package is removed or purged.

### Verify audio modules

```bash
lsmod | grep -E 'snd_soc_sm8150|q6asm|q6adm|cs35l41'
aplay -l
sudo dmesg | grep -Ei 'slim|q6|cs35l41' | tail -30
```

### Fix clipping or distorted sound

The default stream gain is `4106` (≈ -6 dB). To adjust it for the current session:

```bash
echo 4106 | sudo tee /sys/module/q6asm/parameters/nabu_stream_gain_q13
```

Gain values (Q13 linear): `8192` = 0 dB, `5800` ≈ -3 dB, `4106` ≈ -6 dB.

To make the setting permanent:

```bash
echo 'options q6asm nabu_stream_gain_q13=4106' | sudo tee /etc/modprobe.d/nabu-audio.conf
sudo update-initramfs -u
sudo reboot
```

If you still hear clipping, lower the value further (e.g. `3500` ≈ -7.4 dB) or use Easy Effects with a limiter, which addresses level-dependent clipping in the DSP path.

### Audio buffer configuration

If you hear crackling, dropouts or underruns, increase the audio buffer. Edit `/etc/pulse/daemon.conf` (or `~/.config/pulse/daemon.conf`):

```ini
default-fragments = 4
default-fragment-size-msec = 256
```

Available values: `128` ms, `256` ms, `512` ms. `256` ms or `512` ms are recommended for stability.

**Note:** a larger buffer makes audio more stable but also increases audio latency.

Restart the audio server (or reboot):

```bash
pulseaudio -k
# or, on PipeWire systems:
systemctl --user restart pipewire-pulse
```

### If there is no sound at all

```bash
sudo modprobe snd_soc_sm8150
sudo dmesg | tail -50
aplay -l
```

Make sure the `xiaomi-nabu-alsa` package is installed for the correct ALSA UCM profiles:

```bash
sudo apt install xiaomi-nabu-alsa xiaomi-nabu-firmware
```

## Companion feature: power / lid screen toggle

`features/power-screen-toggle/` is an attached daily-use feature for the pad:
a small daemon that makes the power button and lid switch toggle the display
instead of powering off/suspending, with keyboard/mouse/touchpad wake, suspend
countdowns, charging override and automatic panel-recovery cycles.

- Docs & source: [features/power-screen-toggle/](features/power-screen-toggle/)
- Install on the device: `sudo bash features/power-screen-toggle/install.sh`

## Companion feature: charge control

`features/charge-control/` is an attached daily-use feature for the pad:
a small Rust daemon that stops charging above 75% battery and resumes below
50%, protecting the LN8000 charger and battery from staying at full charge
for long periods.

- Docs & source: [features/charge-control/](features/charge-control/)
- Install on the device: `sudo bash features/charge-control/install.sh`

## Repository layout

- `patches/` — kernel patches applied by the build workflow.
- `kernel-build-files/` — extra kernel config and Debian `postinst`/`postrm` scripts.
- `features/power-screen-toggle/` — companion daemon for power/lid screen toggle.
- `features/charge-control/` — battery threshold charging daemon (LN8000, Rust).
- `.github/workflows/build-kernel.yml` — builds the kernel `.deb`/`.tar` package.

## Older README

The previous full installation guide (partitioning, flashing Ubuntu, bootloader setup) is still available here: [README-legacy.md](README-legacy.md)
