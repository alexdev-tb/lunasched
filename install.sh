#!/bin/bash
set -e

echo "Building Lunasched..."
cargo build --release

echo "Stopping service..."
sudo systemctl stop lunasched || true

echo "Installing binaries..."
sudo cp target/release/lunasched-daemon /usr/local/bin/
sudo cp target/release/lunasched /usr/local/bin/

echo "Creating lunasched user..."
if ! id "lunasched" &>/dev/null; then
    sudo useradd -r -s /bin/false -d /var/lib/lunasched lunasched
fi

echo "Creating working directory..."
sudo mkdir -p /var/lib/lunasched
sudo chown lunasched:lunasched /var/lib/lunasched
sudo chmod 750 /var/lib/lunasched

echo "Creating log directory..."
sudo mkdir -p /var/log/lunasched
sudo chown lunasched:lunasched /var/log/lunasched
sudo chmod 750 /var/log/lunasched

echo "Installing systemd service..."
sudo cp lunasched.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable lunasched
sudo systemctl restart lunasched

echo "Installation complete!"
echo "Lunasched is running as a system service."
echo "Use 'lunasched' to interact with the daemon."
