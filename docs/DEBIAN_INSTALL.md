# Debian Package Installation Guide

## Installation

### Install the Package

```bash
sudo dpkg -i target/debian/lunasched_1.2.0-1_amd64.deb
```

If you encounter dependency issues, run:

```bash
sudo apt-get install -f
```

### Post-Installation

The package automatically:
- Creates the `lunasched` system user
- Sets up directories: `/etc/lunasched`, `/var/lib/lunasched`, `/var/log/lunasched`, `/var/run/lunasched`
- Installs default configuration to `/etc/lunasched/config.yaml`
- Enables the systemd service (but doesn't start it)

### Start the Service

```bash
sudo systemctl start lunasched
sudo systemctl status lunasched
```

### Configuration

Edit the configuration file:

```bash
sudo nano /etc/lunasched/config.yaml
```

After making changes, restart the service:

```bash
sudo systemctl restart lunasched
```

### Usage

Add jobs using the CLI:

```bash
lunasched add --name backup --schedule "at 02:00" --command /path/to/backup.sh
lunasched list
```

See `/usr/share/doc/lunasched/PRODUCTION_JOBS.md` for comprehensive examples.

## Uninstallation

### Remove Package (keep config)

```bash
sudo dpkg -r lunasched
```

### Purge Package (remove everything)

```bash
sudo dpkg -P lunasched
```

This will remove:
- All binaries
- Database (`/var/lib/lunasched`)
- Logs (`/var/log/lunasched`)
- The `lunasched` user
- Configuration files

## Package Contents

- **Binaries**: `/usr/local/bin/lunasched`, `/usr/local/bin/lunasched-daemon`
- **Systemd Service**: `/lib/systemd/system/lunasched.service`
- **Configuration**: `/etc/lunasched/config.yaml` (conffile)
- **Documentation**: `/usr/share/doc/lunasched/`

## Building from Source

If you want to rebuild the package:

```bash
cargo build --release
cd daemon && cargo deb --no-strip
```

The package will be created at `target/debian/lunasched_1.2.0-1_amd64.deb`.

## Package Maintainer Scripts

The package includes:

- **postinst**: Creates user, directories, sets permissions, enables service
- **prerm**: Stops and disables service before removal
- **postrm**: Cleans up data and user on purge

## Distribution

You can distribute the `.deb` package to other Debian/Ubuntu systems:

```bash
# Copy to other systems
scp target/debian/lunasched_1.2.0-1_amd64.deb user@remote:/tmp/

# Install on remote system
ssh user@remote 'sudo dpkg -i /tmp/lunasched_1.2.0-1_amd64.deb'
```

## Requirements

- Debian/Ubuntu-based system
- systemd
- No additional dependencies (statically linked Rust binary)
