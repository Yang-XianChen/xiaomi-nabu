#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

if [[ $EUID -ne 0 ]]; then
    echo "Please run as root: sudo bash install.sh" >&2
    exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
    for candidate in "$HOME/.cargo/bin/cargo" "/home/${SUDO_USER:-none}/.cargo/bin/cargo"; do
        if [[ -x "$candidate" ]]; then
            export PATH="$(dirname "$candidate"):$PATH"
            break
        fi
    done
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not found in PATH; install the Rust toolchain first" >&2
    exit 1
fi

cargo build --release --manifest-path screen-toggle-rs/Cargo.toml

install -m 755 screen-toggle-rs/target/release/screen-toggle-daemon /usr/local/sbin/screen-toggle-daemon
install -D -m 644 screen-toggle.service /etc/systemd/system/screen-toggle.service
install -D -m 644 screen-toggle-resume.service /etc/systemd/system/screen-toggle-resume.service
install -D -m 644 10-screen-toggle.conf /etc/systemd/logind.conf.d/10-screen-toggle.conf

systemctl daemon-reload
systemctl enable --now screen-toggle.service
systemctl restart screen-toggle.service
systemctl enable screen-toggle-resume.service

if [[ -n "${SUDO_USER:-}" && "$SUDO_USER" != "root" ]]; then
    user="$SUDO_USER"
    uid="$(id -u "$user")"
    runtime="/run/user/$uid"
    bus="unix:path=$runtime/bus"
    for key in power-button-action lid-close-ac-action lid-close-battery-action; do
        runuser -u "$user" -- env \
            XDG_RUNTIME_DIR="$runtime" \
            DBUS_SESSION_BUS_ADDRESS="$bus" \
            gsettings set org.gnome.settings-daemon.plugins.power "$key" nothing \
            >/dev/null 2>&1 || true
    done
fi

echo
echo "Installed. Status:"
systemctl status screen-toggle.service --no-pager || true
echo
echo "Note: a reboot is recommended so systemd-logind picks up the new"
echo "10-screen-toggle.conf override."
