# xiaomi-nabu — Kernel 6.14.11 (audio fixes)

Linux kernel and modules for the Xiaomi Pad 5 (nabu). This release is based on the stable **6.14.11** kernel with audio fixes. Kernel 6.17 is not deprecated, but the upstream 6.17 build could not be tested on my device; 6.14.11 is the tested release.

## About kernel 6.17

Kernel 6.17 is not deprecated. It is simply not the tested release here: the upstream 6.17 build does not run on my device, so it could not be tested. If it works on your device, you can still build it with the repository workflow.

## Latest release

- Kernel: `6.14.11-nabu-tmm+`
- Package: `xiaomi-nabu-linux-6.14_6.14.11-nabu-tmm+_arm64.deb`
- Download: <https://github.com/Yang-XianChen/xiaomi-nabu/releases/latest>

## Apply the new kernel

On your Xiaomi Pad 5 Ubuntu system:

```bash
# 1. Download the package
curl -L -o xiaomi-nabu-linux-6.14.deb \
  https://github.com/Yang-XianChen/xiaomi-nabu/releases/latest/download/xiaomi-nabu-linux-6.14_6.14.11-nabu-tmm+_arm64.deb

# 2. Install it
sudo apt install ./xiaomi-nabu-linux-6.14.deb
# or: sudo dpkg -i xiaomi-nabu-linux-6.14.deb

# 3. Reboot
sudo reboot
```

After reboot, verify the running kernel:

```bash
uname -r
# expected output: 6.14.11-nabu-tmm
```

The package's `postinst` script automatically builds the UKI EFI file and copies it to the ESP partition. If you use a boot manager, select the new kernel entry after reboot.

### Rollback

Keep the previous kernel package installed. If the new kernel does not boot, select the old entry from your boot manager, or reinstall the previous `.deb`:

```bash
sudo dpkg -i xiaomi-nabu-linux-6.14_<old-version>_arm64.deb
```

## Sound fixes and configuration

This kernel includes these audio fixes:

- SLIMbus QMI service wait with retries — fixes audio staying dead after a slow ADSP boot.
- q6adm / q6asm stream close fixes — prevents DSP session leaks and failed re-opens.
- CS35L41 PUP/PDN errors demoted from errors to warnings.
- ASM stream master gain control (`nabu_stream_gain_q13`), default **-6 dB** to avoid DSP clipping.

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

## Repository layout

- `patches/` — kernel patches applied by the build workflow.
- `kernel-build-files/` — extra kernel config and Debian `postinst`/`postrm` scripts.
- `.github/workflows/build-kernel.yml` — builds the kernel `.deb`/`.tar` package.

## Older README

The previous full installation guide (partitioning, flashing Ubuntu, bootloader setup) is still available here: [README-legacy.md](README-legacy.md)
