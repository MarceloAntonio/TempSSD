# SSD Temperature Monitor

A Windows CLI written in Rust that watches your drive's temperature and only updates the screen when something changes. Every change goes to a log file silently in the background.

Vibe coded with [Antigravity](https://antigravity.dev).

## Requirements

- Windows 10 or 11
- Administrator privileges (Windows requires it to read hardware sensor data)
- No runtime dependencies — just the compiled `.exe`

## Building

```
cargo build --release
```

The binary ends up at `target\release\monitor_ssd.exe`.

## Usage

```
monitor_ssd.exe [options]
```

Running without arguments monitors Disk 0 and shows the current temperature right away, without waiting for it to change first.

### Options

| Flag | Description |
|------|-------------|
| `-d`, `--disk` | Opens an arrow-key selector listing all connected drives with their name, type, and size. Navigate with the arrow keys and press Enter. |
| `-i SECONDS`, `--interval SECONDS` | How often to check for a temperature change. Default is 3 seconds. |
| `-l PATH`, `--log PATH` | Where to write the log file. Defaults to `ssd_temp.log` in the working directory. |

### Examples

```
monitor_ssd.exe
monitor_ssd.exe -d
monitor_ssd.exe -d -i 5
monitor_ssd.exe --interval 10 --log D:\logs\ssd.log
```

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
