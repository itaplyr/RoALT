//! Bandwidth limiting on Linux is not implemented (a Windows QoS policy in the original).
//! The settings and boot tracking remain so the UI can show the option as unavailable.
#![allow(dead_code)]


use crate::error::Result;

pub const MIN_KBPS: u32 = 64;
pub const MAX_KBPS: u32 = 10_000_000;

pub fn script(_kbps: u32) -> Result<String> {
    Ok(String::new())
}

pub fn apply(_kbps: u32) -> Result<()> {
    Ok(())
}

pub fn boot_ms(now_ms: i64) -> i64 {
    let uptime = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .unwrap_or(0.0);
    now_ms - (uptime * 1000.0) as i64
}

pub fn applied_this_boot(applied_boot_ms: i64, boot_ms: i64) -> bool {
    applied_boot_ms >= boot_ms
}
