#!/usr/bin/env bash
#
# 安装充电控制守护程序（Rust 版，需要 root 与 cargo）
#
# 用法: sudo ./install.sh
#
set -euo pipefail

cd "$(dirname "$0")/charge-control-rs"

if ! command -v cargo >/dev/null 2>&1; then
    echo "未找到 cargo，请先安装 Rust 工具链" >&2
    exit 1
fi

cargo build --release

install -m 755 target/release/charge-control /usr/local/sbin/charge-control
install -m 644 ../charge-control.service /etc/systemd/system/charge-control.service

systemctl daemon-reload
systemctl enable --now charge-control

echo "已安装并启动 charge-control 服务。"
echo "查看状态: systemctl status charge-control"
echo "查看日志: journalctl -u charge-control -f"
