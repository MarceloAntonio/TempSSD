use std::{os::windows::process::CommandExt, path::PathBuf, process::Command};

use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;
use wmi::{COMLibrary, WMIConnection};

use monitor_ssd::{Args, Disk, disk_selector, monitor_loop};

// ─── WMI disk listing ────────────────────────────────────────────────────────

#[derive(Deserialize, Debug, Clone)]
struct WmiDisk {
    #[serde(rename = "DeviceId")]
    device_id: String,
    #[serde(rename = "FriendlyName")]
    friendly_name: String,
    #[serde(rename = "MediaType")]
    media_type: u16,
    #[serde(rename = "Size")]
    size: Option<u64>,
}

fn get_all_disks(wmi: &WMIConnection) -> Result<Vec<Disk>> {
    let mut raw: Vec<WmiDisk> = wmi
        .raw_query("SELECT DeviceId, FriendlyName, MediaType, Size FROM MSFT_PhysicalDisk")
        .context("Failed to query physical disks. Run as Administrator.")?;
    raw.sort_by_key(|d| d.device_id.parse::<u32>().unwrap_or(u32::MAX));
    Ok(raw
        .into_iter()
        .map(|d| Disk {
            device_id: d.device_id,
            friendly_name: d.friendly_name,
            media_type: match d.media_type {
                3 => "HDD".to_string(),
                4 => "SSD".to_string(),
                5 => "SCM".to_string(),
                _ => "Unknown".to_string(),
            },
            size_gb: d
                .size
                .filter(|&b| b > 0)
                .map(|b| b as f64 / 1_073_741_824.0)
                .unwrap_or(0.0),
        })
        .collect())
}

// ─── Temperature via PowerShell ──────────────────────────────────────────────

fn get_temperature(disk_id: &str) -> Option<u8> {
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

    monitor_loop(&disk_id, &disk_name, args.interval, &log_path, get_temperature)?;
    Ok(())
}
