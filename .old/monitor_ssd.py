import time
import subprocess
import os
import ctypes
import sys
import argparse
import msvcrt

kernel32 = ctypes.windll.kernel32
kernel32.SetConsoleMode(kernel32.GetStdHandle(-11), 7)

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
DEFAULT_LOG = os.path.join(SCRIPT_DIR, "ssd_temp.log")

C_RESET  = "\033[0m"
C_CYAN   = "\033[96m"
C_YELLOW = "\033[93m"
C_GREEN  = "\033[92m"
C_RED    = "\033[91m"
C_GRAY   = "\033[90m"
C_WHITE  = "\033[97m"
C_BOLD   = "\033[1m"


def run_ps(command: str, timeout: int = 10) -> str | None:
    startupinfo = subprocess.STARTUPINFO()
    startupinfo.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    try:
        result = subprocess.run(
            ["powershell", "-NoProfile", "-Command", command],
            capture_output=True,
            text=True,
            startupinfo=startupinfo,
            timeout=timeout,
        )
        if result.returncode == 0:
            return result.stdout.strip()
        return None
    except (subprocess.TimeoutExpired, Exception):
        return None


def get_all_disks() -> list[dict]:
    output = run_ps(
        "Get-PhysicalDisk | Select-Object DeviceId, FriendlyName, MediaType, Size "
        "| ConvertTo-Json -Compress"
    )
    if not output:
        return []
    import json
    data = json.loads(output)
    if isinstance(data, dict):
        data = [data]
    disks = []
    for d in data:
        size_gb = round(d.get("Size", 0) / (1024 ** 3), 1) if d.get("Size") else 0
        disks.append({
            "id":    str(d.get("DeviceId", "?")),
            "name":  d.get("FriendlyName", "Unknown"),
            "type":  d.get("MediaType", "Unspecified"),
            "size":  size_gb,
        })
    return sorted(disks, key=lambda x: x["id"])


def get_temperature(disk_id: str) -> int | None:
    output = run_ps(
        f"(Get-PhysicalDisk | Where-Object DeviceId -eq {disk_id} "
        f"| Get-StorageReliabilityCounter).Temperature"
    )
    if output and output.isdigit():
        return int(output)
    return None


def build_temp_bar(temp: int) -> str:
    if temp < 40:
        color = C_GREEN
    elif temp <= 55:
        color = C_YELLOW
    else:
        color = C_RED
    filled = min(20, int((temp / 100) * 20))
    bar = "█" * filled + "░" * (20 - filled)
    return f"{color}[{bar}] {temp}°C{C_RESET}"


def print_header():
    print(f"{C_CYAN}{'':=<54}{C_RESET}")
    print(f"{C_CYAN}{C_BOLD}{'SSD TEMPERATURE MONITOR':^54}{C_RESET}")
    print(f"{C_CYAN}{'':=<54}{C_RESET}")


def disk_selector(disks: list[dict]) -> dict:
    selected = 0

    def render(sel):
        sys.stdout.write("\033[H\033[J")
        print_header()
        print(f"\n  {C_WHITE}Select a disk to monitor:{C_RESET}")
        print(f"  {C_GRAY}Use ↑ ↓ to navigate, Enter to confirm.{C_RESET}\n")
        for i, disk in enumerate(disks):
            prefix = f"{C_CYAN}▶ {C_RESET}" if i == sel else "  "
            style  = C_BOLD if i == sel else C_GRAY
            print(
                f"  {prefix}{style}Disk {disk['id']:<4}{C_RESET}"
                f"  {C_WHITE}{disk['name']:<35}{C_RESET}"
                f"  {C_YELLOW}{disk['type']:<12}{C_RESET}"
                f"  {C_GREEN}{disk['size']} GB{C_RESET}"
            )
        print()

    render(selected)

    while True:
        key = msvcrt.getch()
        if key == b"\xe0":
            arrow = msvcrt.getch()
            if arrow == b"H":
                selected = (selected - 1) % len(disks)
            elif arrow == b"P":
                selected = (selected + 1) % len(disks)
            render(selected)
        elif key in (b"\r", b"\n"):
            return disks[selected]


def monitor_loop(disk_id: str, disk_name: str, interval: int, log_path: str):
    sys.stdout.write("\033[H\033[J")
    print_header()
    print(f"\n  Monitoring: {C_YELLOW}{disk_name}{C_RESET}  {C_GRAY}(Disk {disk_id}){C_RESET}")
    print(f"  Log file:   {C_YELLOW}{log_path}{C_RESET}")
    print(f"  {C_GRAY}Press Ctrl+C to quit.{C_RESET}\n")

    last_temp = None

    try:
        while True:
            temp = get_temperature(disk_id)

            if temp is not None and temp != last_temp:
                now = time.strftime("%H:%M:%S")

                with open(log_path, "a", encoding="utf-8") as f:
                    f.write(f"[{now}] Disk {disk_id} ({disk_name}) | Temperature: {temp}°C\n")

                bar = build_temp_bar(temp)
                print(f"\r  [{C_GRAY}{now}{C_RESET}] {bar}   ", end="", flush=True)

                last_temp = temp

            time.sleep(interval)

    except KeyboardInterrupt:
        print(f"\n\n  {C_CYAN}Monitor stopped. Goodbye!{C_RESET}\n")
        sys.exit(0)


def main():
    parser = argparse.ArgumentParser(
        prog="monitor_ssd",
        description="Real-time SSD temperature monitor for Windows.",
        formatter_class=argparse.RawTextHelpFormatter,
    )
    parser.add_argument(
        "-d", "--disk",
        action="store_true",
        help="Show an interactive disk selector instead of defaulting to Disk 0.",
    )
    parser.add_argument(
        "-i", "--interval",
        type=int,
        default=3,
        metavar="SECONDS",
        help="Polling interval in seconds (default: 3).",
    )
    parser.add_argument(
        "-l", "--log",
        default=DEFAULT_LOG,
        metavar="PATH",
        help=f"Path to the log file (default: {DEFAULT_LOG}).",
    )
    args = parser.parse_args()

    if args.disk:
        print(f"{C_CYAN}Fetching disk list...{C_RESET}")
        disks = get_all_disks()
        if not disks:
            print(f"{C_RED}No disks found. Make sure you are running as Administrator.{C_RESET}")
            sys.exit(1)
        disk = disk_selector(disks)
        disk_id   = disk["id"]
        disk_name = disk["name"]
    else:
        disk_id = "0"
        raw = run_ps("(Get-PhysicalDisk | Where-Object DeviceId -eq 0).FriendlyName")
        disk_name = raw if raw else "Disk 0"

    monitor_loop(disk_id, disk_name, args.interval, args.log)


if __name__ == "__main__":
    main()
