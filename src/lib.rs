// Shared code used by both the Windows and Linux binaries.
// Platform-specific functions (get_temperature, get_all_disks) live in each binary.

use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::PathBuf,
    time::Duration,
};

use anyhow::Result;
use chrono::Local;
use clap::Parser;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};

// ─── Common disk struct ──────────────────────────────────────────────────────
// Each binary constructs this from its own platform data source.

#[derive(Debug, Clone)]
pub struct Disk {
    pub device_id: String,
    pub friendly_name: String,
    pub media_type: String,
    pub size_gb: f64,
}

// ─── CLI args ────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(about = "Real-time SSD temperature monitor.")]
pub struct Args {
    #[arg(short = 'd', long, help = "Interactive disk selector")]
    pub disk: bool,

    #[arg(
        short = 'i',
        long,
        default_value = "3",
        value_name = "SECONDS",
        help = "Polling interval in seconds (default: 3)"
    )]
    pub interval: u64,

    #[arg(short = 'l', long, value_name = "PATH", help = "Log file path")]
    pub log: Option<PathBuf>,
}

// ─── Raw mode guard ──────────────────────────────────────────────────────────

pub struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Show);
    }
}

// ─── UI helpers ──────────────────────────────────────────────────────────────

pub fn build_temp_bar(temp: u8) -> String {
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

pub fn print_header() {
    let line = "=".repeat(54);
    println!("\x1b[96m{}\x1b[0m", line);
    println!("\x1b[96m\x1b[1m{:^54}\x1b[0m", "SSD TEMPERATURE MONITOR");
    println!("\x1b[96m{}\x1b[0m", line);
}

pub fn display_and_log(
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

    print!("\r  [\x1b[90m{now}\x1b[0m] {}\x1b[K", build_temp_bar(temp));
    io::stdout().flush()?;
    Ok(())
}

// ─── Disk selector UI ────────────────────────────────────────────────────────

pub fn disk_selector(disks: &[Disk]) -> Result<usize> {
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
                "  {}{style}{:<6}\x1b[0m  \x1b[97m{:<35}\x1b[0m  \x1b[93m{:<12}\x1b[0m  \x1b[92m{:.1} GB\x1b[0m",
                prefix,
                disk.device_id,
                disk.friendly_name,
                disk.media_type,
                disk.size_gb,
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

pub fn monitor_loop(
    disk_id: &str,
    disk_name: &str,
    interval: u64,
    log_path: &PathBuf,
    get_temp: impl Fn(&str) -> Option<u8>,
) -> Result<()> {
    let mut stdout = io::stdout();

    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )?;
    print_header();
    println!();
    println!("  Monitoring: \x1b[93m{disk_name}\x1b[0m  \x1b[90m({disk_id})\x1b[0m");
    println!("  Log file:   \x1b[93m{}\x1b[0m", log_path.display());
    println!("  \x1b[90mPress Ctrl+C to quit.\x1b[0m");
    println!();
    stdout.flush()?;

    terminal::enable_raw_mode()?;
    let _guard = RawModeGuard;
    execute!(stdout, cursor::Hide)?;

    let mut last_temp: Option<u8> = None;

    match get_temp(disk_id) {
        Some(temp) => display_and_log(temp, &mut last_temp, disk_id, disk_name, log_path)?,
        None => {
            print!("\r  \x1b[90mWaiting for sensor data...\x1b[0m");
            stdout.flush()?;
        }
    }

    let interval_dur = Duration::from_secs(interval);

    loop {
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
            match get_temp(disk_id) {
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

    println!("\r\n\r\n  \x1b[96mMonitor stopped. Goodbye!\x1b[0m\r\n");
    Ok(())
}
