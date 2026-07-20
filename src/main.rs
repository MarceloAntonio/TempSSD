use std::{
    fs::OpenOptions,
    io::{self, Write},
    os::windows::process::CommandExt,
    path::PathBuf,
    process::Command,
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::Local;
use clap::Parser;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};
use serde::Deserialize;
use wmi::{COMLibrary, WMIConnection};

// ─── WMI structs ────────────────────────────────────────────────────────────

#[derive(Deserialize, Debug, Clone)]
struct PhysicalDisk {
    #[serde(rename = "DeviceId")]
    device_id: String,
    #[serde(rename = "FriendlyName")]
    friendly_name: String,
    #[serde(rename = "MediaType")]
    media_type: u16,
    #[serde(rename = "Size")]
    size: Option<u64>,
}

// ─── CLI args ────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "monitor_ssd",
    about = "Real-time SSD temperature monitor for Windows."
)]
struct Args {
    #[arg(short = 'd', long, help = "Interactive disk selector")]
    disk: bool,

    #[arg(
        short = 'i',
        long,
        default_value = "3",
        value_name = "SECONDS",
        help = "Polling interval in seconds (default: 3)"
    )]
    interval: u64,

    #[arg(short = 'l', long, value_name = "PATH", help = "Log file path")]
    log: Option<PathBuf>,
}

// ─── Raw mode guard ──────────────────────────────────────────────────────────

struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Show);
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn build_temp_bar(temp: u8) -> String {
    let color = if temp < 40 {
        "\x1b[92m"
    } else if temp <= 55 {
        "\x1b[93m"
    } else {
        "\x1b[91m"
    };
    let filled = ((temp as usize * 20) / 100).min(20);
    format!(
        "{}[{}{}] {}°C\x1b[0m",
        color,
        "█".repeat(filled),
        "░".repeat(20 - filled),
        temp
    )
}

fn print_header() {
    let line = "=".repeat(54);
    println!("\x1b[96m{}\x1b[0m", line);
    println!("\x1b[96m\x1b[1m{:^54}\x1b[0m", "SSD TEMPERATURE MONITOR");
    println!("\x1b[96m{}\x1b[0m", line);
}

// ─── WMI queries ─────────────────────────────────────────────────────────────

fn get_all_disks(wmi: &WMIConnection) -> Result<Vec<PhysicalDisk>> {
    let mut disks: Vec<PhysicalDisk> = wmi
        .raw_query("SELECT DeviceId, FriendlyName, MediaType, Size FROM MSFT_PhysicalDisk")
        .context("Failed to query physical disks. Run as Administrator.")?;
    disks.sort_by_key(|d| d.device_id.parse::<u32>().unwrap_or(u32::MAX));
    Ok(disks)
}

fn get_temperature(disk_id: &str) -> Option<u8> {
    // MSFT_StorageReliabilityCounter via direct WQL returns empty on most drivers.
    // PowerShell uses CIM association (Get-PhysicalDisk | Get-StorageReliabilityCounter)
    // which is the only reliable path on Windows without raw IOCTL.
    let script = format!(
        "(Get-PhysicalDisk | Where-Object DeviceId -eq {} | Get-StorageReliabilityCounter).Temperature",
        disk_id
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .ok()?;

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u8>()
        .ok()
}

// ─── Display + log ────────────────────────────────────────────────────────────

fn display_and_log(
    temp: u8,
    last: &mut Option<u8>,
    disk_id: &str,
    disk_name: &str,
    log_path: &PathBuf,
) -> Result<()> {
    if Some(temp) == *last {
        return Ok(());
    }
    *last = Some(temp);

    let now = Local::now().format("%H:%M:%S").to_string();

    let mut file = OpenOptions::new().create(true).append(true).open(log_path)?;
    writeln!(file, "[{now}] Disk {disk_id} ({disk_name}) | Temperature: {temp}°C")?;

    // \r goes to column 0; \x1b[K erases to end of line to avoid ghost characters
    print!("\r  [\x1b[90m{now}\x1b[0m] {}\x1b[K", build_temp_bar(temp));
    io::stdout().flush()?;
    Ok(())
}

// ─── Disk selector UI ────────────────────────────────────────────────────────

fn disk_selector(disks: &[PhysicalDisk]) -> Result<usize> {
    let mut stdout = io::stdout();
    let mut selected = 0usize;

    terminal::enable_raw_mode()?;
    let _guard = RawModeGuard;
    execute!(stdout, cursor::Hide)?;

    loop {
        execute!(
            stdout,
            terminal::Clear(ClearType::All),
            cursor::MoveTo(0, 0)
        )?;

        print_header();
        println!();
        println!("  \x1b[97mSelect a disk to monitor:\x1b[0m");
        println!("  \x1b[90mUse \u{2191} \u{2193} to navigate, Enter to confirm.\x1b[0m");
        println!();

        for (i, disk) in disks.iter().enumerate() {
            let (prefix, style) = if i == selected {
                ("\x1b[96m\u{25b6} \x1b[0m", "\x1b[1m\x1b[97m")
            } else {
                ("  ", "\x1b[90m")
            };
            let size_str = disk
                .size
                .filter(|&b| b > 0)
                .map(|b| format!("{:.1} GB", b as f64 / 1_073_741_824.0))
                .unwrap_or_else(|| "? GB".to_string());
            let type_str = match disk.media_type {
                3 => "HDD",
                4 => "SSD",
                5 => "SCM",
                _ => "Unknown",
            };
            println!(
                "  {}{style}Disk {:<4}\x1b[0m  \x1b[97m{:<35}\x1b[0m  \x1b[93m{:<12}\x1b[0m  \x1b[92m{}\x1b[0m",
                prefix,
                disk.device_id,
                disk.friendly_name,
                type_str,
                size_str,
            );
        }
        stdout.flush()?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Up => {
                        selected = if selected == 0 { disks.len() - 1 } else { selected - 1 };
                    }
                    KeyCode::Down => {
                        selected = (selected + 1) % disks.len();
                    }
                    KeyCode::Enter => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // _guard drops here, restoring terminal before exit
                        drop(_guard);
                        println!("\r\n\r\n  \x1b[96mMonitor stopped. Goodbye!\x1b[0m\r\n");
                        std::process::exit(0);
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(selected)
}

// ─── Monitor loop ────────────────────────────────────────────────────────────

fn monitor_loop(
    _wmi: &WMIConnection,
    disk_id: &str,
    disk_name: &str,
    interval: u64,
    log_path: &PathBuf,
) -> Result<()> {
    let mut stdout = io::stdout();

    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )?;
    print_header();
    println!();
    println!("  Monitoring: \x1b[93m{disk_name}\x1b[0m  \x1b[90m(Disk {disk_id})\x1b[0m");
    println!("  Log file:   \x1b[93m{}\x1b[0m", log_path.display());
    println!("  \x1b[90mPress Ctrl+C to quit.\x1b[0m");
    println!();
    stdout.flush()?;

    terminal::enable_raw_mode()?;
    let _guard = RawModeGuard;
    execute!(stdout, cursor::Hide)?;

    let mut last_temp: Option<u8> = None;

    // Show temperature immediately on start without waiting for the first interval
    match get_temperature(disk_id) {
        Some(temp) => display_and_log(temp, &mut last_temp, disk_id, disk_name, log_path)?,
        None => {
            print!("\r  \x1b[90mWaiting for sensor data... (run as Administrator if stuck)\x1b[0m");
            stdout.flush()?;
        }
    }

    let interval_dur = Duration::from_secs(interval);

    loop {
        // poll() blocks for up to interval_dur. True = event arrived, false = timeout.
        if event::poll(interval_dur)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press
                    && key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    break;
                }
            }
        } else {
            match get_temperature(disk_id) {
                Some(temp) => {
                    display_and_log(temp, &mut last_temp, disk_id, disk_name, log_path)?
                }
                None if last_temp.is_none() => {
                    print!("\r  \x1b[90mWaiting for sensor data...\x1b[0m\x1b[K");
                    stdout.flush()?;
                }
                _ => {}
            }
        }
    }

    // _guard drops here: raw mode off, cursor visible.
    // Use \r\n instead of \n so the text aligns correctly after raw mode.
    println!("\r\n\r\n  \x1b[96mMonitor stopped. Goodbye!\x1b[0m\r\n");
    Ok(())
}

// ─── Entry point ─────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let args = Args::parse();
    let log_path = args.log.unwrap_or_else(|| PathBuf::from("ssd_temp.log"));

    let com_lib = COMLibrary::new().context("Failed to initialize COM.")?;
    let wmi = WMIConnection::with_namespace_path("ROOT\\Microsoft\\Windows\\Storage", com_lib)
        .context("Failed to connect to WMI. Run as Administrator.")?;

    let (disk_id, disk_name) = if args.disk {
        let disks = get_all_disks(&wmi)?;
        if disks.is_empty() {
            eprintln!("\x1b[91mNo disks found. Run as Administrator.\x1b[0m");
            std::process::exit(1);
        }
        let idx = disk_selector(&disks)?;
        let d = &disks[idx];
        (d.device_id.clone(), d.friendly_name.clone())
    } else {
        let disks = get_all_disks(&wmi)?;
        let name = disks
            .iter()
            .find(|d| d.device_id == "0")
            .map(|d| d.friendly_name.clone())
            .unwrap_or_else(|| "Disk 0".to_string());
        ("0".to_string(), name)
    };

    monitor_loop(&wmi, &disk_id, &disk_name, args.interval, &log_path)?;
    Ok(())
}
