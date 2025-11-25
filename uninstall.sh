#!/bin/bash
set -e

echo "Stopping and disabling service..."
sudo systemctl stop lunasched || true
sudo systemctl disable lunasched || true

echo "Removing service file..."
sudo rm -f /etc/systemd/system/lunasched.service
sudo systemctl daemon-reload

echo "Removing binaries..."
sudo rm -f /usr/local/bin/lunasched
sudo rm -f /usr/local/bin/lunasched-daemon

echo "Removing data and log directories..."
sudo rm -rf /var/lib/lunasched
sudo rm -rf /var/log/lunasched

echo "Removing lunasched user..."
if id "lunasched" &>/dev/null; then
    sudo userdel lunasched
fi

echo "Uninstallation complete!"
