#!/usr/bin/env bash
#
# Install the power/lid screen-toggle companion feature for xiaomi-nabu.
#
#   * Builds the Rust daemon
#   * Installs /usr/local/sbin/screen-toggle-daemon
#   * Installs systemd units + logind override
#   * Best-effort: sets GNOME power/lid actions to "nothing" for the
#     invoking graphical user
#
# Usage: sudo bash install.sh

set -euo pipefail

cd "$(dirname "$0")"

if [ "$(id -u)" -ne 0 ]; then
    echo "Please run as root: sudo bash install.sh" >&2
    exit 1
fi

echo "==> Building screen-toggle-daemon (release)..."
( cd screen-toggle-rs && cargo build --release )

echo "==> Installing daemon and system files..."
install -d /etc/systemd/logind.conf.d
install -m 755 screen-toggle-rs/target/release/screen-toggle-daemon /usr/local/sbin/screen-toggle-daemon
install -m 644 screen-toggle.service /etc/systemd/system/screen-toggle.service
install -m 644 screen-toggle-resume.service /etc/systemd/system/screen-toggle-resume.service
install -m 644 10-screen-toggle.conf /etc/systemd/logind.conf.d/10-screen-toggle.conf

echo "==> Reloading systemd and starting service..."
systemctl daemon-reload
systemctl enable --now screen-toggle.service
systemctl restart screen-toggle.service

# Best-effort: make GNOME ignore the power button and lid switch (the daemon
# handles them). Requires a graphical user session; failures are non-fatal.
GNOME_USER="${SUDO_USER:-}"
if [ -n "$GNOME_USER" ] && [ "$GNOME_USER" != "root" ]; then
    echo "==> Setting GNOME power/lid actions to 'nothing' for $GNOME_USER ..."
    for key in power-button-action lid-close-ac-action lid-close-battery-action; do
        sudo -u "$GNOME_USER" gsettings set \
            org.gnome.settings-daemon.plugins.power "$key" nothing 2>/dev/null || \
            echo "    (skip $key: no GNOME session for $GNOME_USER)"
    done
fi

echo
echo "Installed. Status:"
systemctl status screen-toggle.service --no-pager || true
echo
echo "Note: a reboot is recommended so systemd-logind picks up the new"
echo "10-screen-toggle.conf override."
