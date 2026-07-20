use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use clap::Parser;

use monitor_ssd::{Args, Disk, disk_selector, monitor_loop};

// ─── Disk listing via /sys/block ─────────────────────────────────────────────

fn get_all_disks() -> Result<Vec<Disk>> {
    let mut disks: Vec<Disk> = Vec::new();

    for entry in fs::read_dir("/sys/block").context("Cannot read /sys/block")? {
        let entry = entry?;
        let dev_name = entry.file_name().to_string_lossy().into_owned();

        // Skip virtual devices: loop, ram, dm, zram, sr
        if dev_name.starts_with("loop")
            || dev_name.starts_with("ram")
            || dev_name.starts_with("dm-")
            || dev_name.starts_with("zram")
            || dev_name.starts_with("sr")
        {
            continue;
        }

        let dev_path = entry.path();

        // Skip partitions (they have a stat file but no queue/rotational)
        if !dev_path.join("queue/rotational").exists() {
            continue;
        }

        let model = fs::read_to_string(dev_path.join("device/model"))
            .unwrap_or_default()
            .trim()
            .to_string();

        let size_bytes: u64 = fs::read_to_string(dev_path.join("size"))
            .unwrap_or_default()
            .trim()
            .parse()
            .unwrap_or(0);
        let size_gb = (size_bytes * 512) as f64 / 1_073_741_824.0;

        let rotational: u8 = fs::read_to_string(dev_path.join("queue/rotational"))
            .unwrap_or_default()
            .trim()
            .parse()
            .unwrap_or(1);
        let media_type = if dev_name.starts_with("nvme") {
            "NVMe"
        } else if rotational == 0 {
            "SSD"
        } else {
            "HDD"
        }
        .to_string();

        disks.push(Disk {
            device_id: dev_name,
            friendly_name: if model.is_empty() { "Unknown".to_string() } else { model },
            media_type,
            size_gb,
        });
    }

    disks.sort_by(|a, b| a.device_id.cmp(&b.device_id));
    Ok(disks)
}

// ─── Temperature reading ──────────────────────────────────────────────────────

fn read_millicelsius(path: &Path) -> Option<u8> {
    fs::read_to_string(path)
        .ok()?
        .trim()
        .parse::<i64>()
        .ok()
        .map(|mc| (mc / 1000).clamp(0, 255) as u8)
}

fn get_temperature(disk_id: &str) -> Option<u8> {
    // Strategy 1: NVMe hwmon sysfs (no extra tools needed, kernel ≥ 5.x)
    // Path: /sys/class/nvme/{id}/hwmon*/temp1_input
    if disk_id.starts_with("nvme") {
        let nvme_path = Path::new("/sys/class/nvme").join(disk_id);
        if let Ok(entries) = fs::read_dir(&nvme_path) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                if name.to_string_lossy().starts_with("hwmon") {
                    let temp_path = entry.path().join("temp1_input");
                    if let Some(t) = read_millicelsius(&temp_path) {
                        return Some(t);
                    }
                }
            }
        }
    }

    // Strategy 2: hwmon scan — find the hwmon whose "name" matches the block device
    // Covers many SATA SSDs and some NVMe controllers
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            let hwmon_path = entry.path();
            let hwmon_name = fs::read_to_string(hwmon_path.join("name"))
                .unwrap_or_default()
                .trim()
                .to_lowercase();

            if hwmon_name.contains(disk_id) || hwmon_name.contains("nvme") && disk_id.starts_with("nvme") {
                let temp_path = hwmon_path.join("temp1_input");
                if let Some(t) = read_millicelsius(&temp_path) {
                    return Some(t);
                }
            }
        }
    }

    // Strategy 3: smartctl fallback (requires smartmontools to be installed)
    // Works for SATA SSDs and HDDs that don't expose hwmon
    let dev_path = format!("/dev/{disk_id}");
    let output = Command::new("smartctl")
        .args(["-A", &dev_path])
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("temperature_celsius") || lower.contains("temperature") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(val) = parts.last() {
                if let Ok(t) = val.parse::<u8>() {
                    return Some(t);
                }
            }
        }
    }

    None
}

// ─── Entry point ─────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let args = Args::parse();
    let log_path = args.log.unwrap_or_else(|| PathBuf::from("ssd_temp.log"));

    let (disk_id, disk_name) = if args.disk {
        let disks = get_all_disks()?;
        if disks.is_empty() {
            eprintln!("\x1b[91mNo disks found in /sys/block.\x1b[0m");
            std::process::exit(1);
        }
        let idx = disk_selector(&disks)?;
        let d = &disks[idx];
        (d.device_id.clone(), d.friendly_name.clone())
    } else {
        // Default: first NVMe drive, or first disk found
        let disks = get_all_disks()?;
        let default = disks
            .iter()
            .find(|d| d.device_id.starts_with("nvme"))
            .or_else(|| disks.first())
            .cloned();

        match default {
            Some(d) => (d.device_id, d.friendly_name),
            None => {
                eprintln!("\x1b[91mNo disks found. Run as root if needed.\x1b[0m");
                std::process::exit(1);
            }
        }
    };

    monitor_loop(&disk_id, &disk_name, args.interval, &log_path, get_temperature)?;
    Ok(())
}
