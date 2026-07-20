# SSD Temperature Monitor

A CLI utility written in Rust that watches your drive's temperature and only updates the screen when something changes. Every change goes to a log file silently in the background.

Vibe coded with [Antigravity](https://antigravity.dev).

## Requirements

- **Windows**: Windows 10/11, Administrator privileges (required to read hardware sensor data via PowerShell/WMI).
- **Linux**: Root privileges may be required depending on the drive type (reads from `/sys/class/hwmon`, `/sys/block`, or uses `smartctl` as fallback).
- No runtime dependencies — just the compiled binaries.

## Building

The project is split into two separate binaries to handle platform-specific logic cleanly.

```bash
# Build for Windows (must be run on Windows)
cargo build --bin monitor_ssd_win --release

# Build for Linux (must be run on Linux or WSL)
cargo build --bin monitor_ssd_linux --release
```

The binaries will be located at `target/release/monitor_ssd_win.exe` and `target/release/monitor_ssd_linux`.

## Usage

```bash
# Windows
monitor_ssd_win.exe [options]

# Linux
./monitor_ssd_linux [options]
```

Running without arguments monitors Disk 0 (or the first NVMe drive on Linux) and shows the current temperature right away.

### Options

| Flag | Description |
|------|-------------|
| `-d`, `--disk` | Opens an arrow-key selector listing all connected drives with their name, type, and size. Navigate with the arrow keys and press Enter. |
| `-i SECONDS`, `--interval SECONDS` | How often to check for a temperature change. Default is 3 seconds. |
| `-l PATH`, `--log PATH` | Where to write the log file. Defaults to `ssd_temp.log` in the working directory. |

### Examples

```bash
monitor_ssd_win.exe -d
monitor_ssd_linux -d -i 5
```

## Legacy Python Version

The original Python version of this tool is preserved in the `legacy_python/` directory. It works identically on Windows but requires Python 3.10+ to run. It serves as historical reference and is no longer actively updated.

## What it looks like

The temperature bar changes color based on how hot the drive is:

- Green: below 40°C
- Yellow: 40°C to 55°C
- Red: above 55°C

The screen shows one line at a time and rewrites it in place rather than scrolling. The log file records every change with a timestamp:

```
[09:31:14] Disk 0 (Samsung SSD 980 PRO 1TB) | Temperature: 38°C
[09:47:02] Disk 0 (Samsung SSD 980 PRO 1TB) | Temperature: 41°C
```

Press Ctrl+C to stop. The terminal is restored cleanly.
