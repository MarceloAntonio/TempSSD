use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::PathBuf,
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

#[derive(Deserialize, Debug)]
struct ReliabilityCounter {
    #[serde(rename = "Temperature")]
    temperature: Option<u8>,
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
// Ensures raw mode and cursor are restored even if an early return or panic occurs.

struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Show);
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn media_type_str(mt: u16) -> &'static str {
    match mt {
        3 => "HDD",
        4 => "SSD",
        5 => "SCM",
        _ => "Unknown",
    }
}

fn format_size(bytes: Option<u64>) -> String {
    bytes
        .filter(|&b| b > 0)
        .map(|b| format!("{:.1} GB", b as f64 / 1_073_741_824.0))
        .unwrap_or_else(|| "? GB".to_string())
}

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

fn get_temperature(wmi: &WMIConnection, disk_id: &str) -> Result<Option<u8>> {
    let query = format!(
        "SELECT Temperature FROM MSFT_StorageReliabilityCounter WHERE DeviceId = '{disk_id}'"
    );
    let results: Vec<ReliabilityCounter> = wmi
        .raw_query(query)
        .context("Failed to query temperature. Run as Administrator.")?;
    Ok(results.first().and_then(|r| r.temperature))
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

    // \x1b[K erases from cursor to end of line so shorter lines don't leave ghost chars
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
            println!(
                "  {}{style}Disk {:<4}\x1b[0m  \x1b[97m{:<35}\x1b[0m  \x1b[93m{:<12}\x1b[0m  \x1b[92m{}\x1b[0m",
                prefix,
                disk.device_id,
                disk.friendly_name,
                media_type_str(disk.media_type),
                format_size(disk.size),
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
                        println!("\n\n  \x1b[96mMonitor stopped. Goodbye!\x1b[0m\n");
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
    wmi: &WMIConnection,
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
    if let Some(temp) = get_temperature(wmi, disk_id)? {
        display_and_log(temp, &mut last_temp, disk_id, disk_name, log_path)?;
    }

    let interval_dur = Duration::from_secs(interval);

    loop {
        // poll() blocks for up to `interval_dur`. If an event arrives first it returns true.
        // If the timeout elapses with no events it returns false — that's when we check temp.
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
            if let Some(temp) = get_temperature(wmi, disk_id)? {
                display_and_log(temp, &mut last_temp, disk_id, disk_name, log_path)?;
            }
        }
    }

    // _guard drops here: raw mode off, cursor visible
    println!("\n\n  \x1b[96mMonitor stopped. Goodbye!\x1b[0m\n");
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
