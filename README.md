# SSD Temperature Monitor

A Python CLI for Windows that watches your drive's temperature and only prints to the screen when something actually changes. Everything else goes to a log file.

Vibe coded with [Antigravity](https://antigravity.dev).

## Requirements

- Windows 10 or 11
- Python 3.10+
- Administrator privileges (Windows requires it for hardware sensor access)
- No third-party packages needed

## Usage

```
python monitor_ssd.py [options]
```

Running it without arguments monitors Disk 0 and starts showing the temperature immediately. No waiting for the first change.

### Options

| Flag | Description |
|------|-------------|
| `-d`, `--disk` | Opens an arrow-key selector listing all connected drives with their name, type, and size. Pick one with the arrow keys and press Enter. |
| `-i SECONDS`, `--interval SECONDS` | How often to poll for a temperature change. Default is 3 seconds. |
| `-l PATH`, `--log PATH` | Where to write the log file. Defaults to `ssd_temp.log` in the same folder as the script. |

### Examples

```
python monitor_ssd.py
python monitor_ssd.py -d
python monitor_ssd.py -d -i 5
python monitor_ssd.py --interval 10 --log D:\logs\ssd.log
```

## What it looks like

The temperature bar changes color depending on how hot the drive is running:

- Green: below 40°C, normal
- Yellow: 40°C to 55°C, worth keeping an eye on
- Red: above 55°C, getting hot

The screen always shows one line, the current reading. It rewrites itself in place rather than scrolling. The log file records every change with a timestamp:

```
[09:31:14] Disk 0 (Samsung SSD 980 PRO 1TB) | Temperature: 38°C
[09:47:02] Disk 0 (Samsung SSD 980 PRO 1TB) | Temperature: 41°C
```

Press Ctrl+C to stop. It exits cleanly.

## Notes

The tool reads temperature through `Get-StorageReliabilityCounter` in PowerShell, which is the same source most Windows hardware monitors use. Some NVMe drives report temperature, some do not, depending on the driver. If you see nothing after starting, that is usually a permissions issue (run as Administrator) or the drive not exposing the sensor.
