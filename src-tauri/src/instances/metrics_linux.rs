//! Per-client CPU, memory and I/O from `/proc` on Linux. GPU counters are not wired up yet.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::process::{descendants, start_time};
use crate::model::{ClientMetrics, Limits, SystemUsage};

const CACHE: Duration = Duration::from_secs(2);
const MB: f64 = 1024.0 * 1024.0;
const PAGE: u64 = 4096;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    /// Kernel + user time in clock ticks.
    pub cpu: u64,
    pub working_set: u64,
    pub private: u64,
    pub read: u64,
    pub write: u64,
}

struct Last {
    at: Instant,
    t0: u64,
    r: Reading,
}

pub struct Sampler {
    last: Mutex<HashMap<String, Last>>,
    cache: Mutex<Option<(Instant, Vec<ClientMetrics>)>>,
    /// The PC's last sample and its CPU times (idle, total), for `system`.
    system: Mutex<Option<(Instant, SystemUsage, (u64, u64))>>,
}

impl Sampler {
    pub fn new() -> Sampler {
        Sampler { last: Mutex::new(HashMap::new()), cache: Mutex::new(None), system: Mutex::new(None) }
    }

    /// How busy the whole PC is, reused for `CACHE`; None if `/proc` doesn't answer.
    pub fn system(&self) -> Option<SystemUsage> {
        let mut last = self.system.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, usage, _)) = last.as_ref()
            && at.elapsed() < CACHE
        {
            return Some(*usage);
        }
        let times = cpu_times()?;
        let (used, total) = memory()?;
        let usage = SystemUsage {
            cpu_pct: last.as_ref().and_then(|(_, _, before)| busy_pct(*before, times)),
            mem_used_mb: used as f64 / MB,
            mem_total_mb: total as f64 / MB,
        };
        *last = Some((Instant::now(), usage, times));
        Some(usage)
    }

    pub fn sample(&self, clients: &[(String, u32, u64, Limits)]) -> Vec<ClientMetrics> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, m)) = cache.as_ref()
            && at.elapsed() < CACHE
            && m.len() == clients.len()
            && m.iter().zip(clients).all(|(m, c)| m.instance_id == c.0 && m.pid == c.1)
        {
            return m.clone();
        }
        let cores = std::thread::available_parallelism()
            .inspect_err(|e| log::debug!("metrics: core count unknown, using 1: {e}"))
            .map_or(1, |n| n.get()) as f64;
        // SAFETY: sysconf has no preconditions; a failed call uses the usual 100 Hz.
        let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        let hz = if hz > 0 { hz as f64 } else { 100.0 };
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        last.retain(|id, _| clients.iter().any(|c| c.0 == *id));
        let now = Instant::now();
        let mut out = Vec::new();
        for (id, pid, t0, _) in clients {
            let Some(r) = read(*pid, *t0) else { continue };
            let prev = last.get(id).filter(|l| l.t0 == *t0);
            let secs = prev.map(|l| now.duration_since(l.at).as_secs_f64()).filter(|&s| s > 0.0);
            let rate = |f: fn(&Reading) -> u64| prev.zip(secs).map(|(l, s)| f(&r).saturating_sub(f(&l.r)) as f64 / s);
            out.push(ClientMetrics {
                instance_id: id.clone(),
                pid: *pid,
                cpu_pct: rate(|r| r.cpu).map(|t| (t / hz / cores * 100.0).min(100.0)),
                // Linux reports PSS for both columns: RSS would count shared libraries and the
                // engine image once per client, which made a headless client look tiny when the
                // tracked cage process was measured alone and enormous when the tree was summed.
                memory_mb: r.private as f64 / MB,
                private_mb: r.private as f64 / MB,
                read_bps: rate(|r| r.read),
                write_bps: rate(|r| r.write),
                gpu_pct: None,
                gpu_mem_mb: None,
            });
            last.insert(id.clone(), Last { at: now, t0: *t0, r });
        }
        *cache = Some((now, out.clone()));
        out
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

fn cpu_times() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/stat").ok()?;
    let line = text.lines().next()?.strip_prefix("cpu")?;
    let values: Vec<u64> = line.split_whitespace().filter_map(|v| v.parse().ok()).collect();
    if values.len() < 4 { return None; }
    let idle = values[3] + values.get(4).copied().unwrap_or(0);
    Some((idle, values.iter().sum()))
}

fn memory() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") { total = v.split_whitespace().next()?.parse::<u64>().ok().map(|k| k * 1024); }
        else if let Some(v) = line.strip_prefix("MemAvailable:") { available = v.split_whitespace().next()?.parse::<u64>().ok().map(|k| k * 1024); }
    }
    Some((total?.saturating_sub(available?), total?))
}

fn busy_pct(before: (u64, u64), now: (u64, u64)) -> Option<f64> {
    let total = now.1.checked_sub(before.1).filter(|&t| t > 0)?;
    let idle = now.0.saturating_sub(before.0).min(total);
    Some((total - idle) as f64 / total as f64 * 100.0)
}

/// One reading of the tracked process and all of its descendants, only while the tracked process
/// is still the one started at `t0`.
///
/// A headless Cordial client is `cage`, whose child is the actual `cordial-run` engine. Reading
/// only the tracked PID reported the cage process (~230 MB RSS / ~40 MB PSS) instead of the
/// client. Summing PSS is safe: a shared page is divided between its mappers, so the total is
/// still counted once.
pub fn read(pid: u32, t0: u64) -> Option<Reading> {
    if start_time(pid) != Some(t0) {
        return None;
    }
    let mut family = vec![pid];
    family.extend(descendants(pid));
    let mut out = Reading { cpu: 0, working_set: 0, private: 0, read: 0, write: 0 };
    let mut seen = false;
    for p in family {
        let Some(r) = read_one(p) else { continue };
        seen = true;
        out.cpu = out.cpu.saturating_add(r.cpu);
        out.working_set = out.working_set.saturating_add(r.working_set);
        out.private = out.private.saturating_add(r.private);
        out.read = out.read.saturating_add(r.read);
        out.write = out.write.saturating_add(r.write);
    }
    seen.then_some(out)
}

fn read_one(pid: u32) -> Option<Reading> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after = stat.rsplit_once(')')?.1;
    let f: Vec<&str> = after.split_whitespace().collect();
    // After `)`, index 0 is field 3 (state); utime and stime are fields 14 and 15.
    let cpu = f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?;
    let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
    let pages: Vec<u64> = statm.split_whitespace().filter_map(|s| s.parse().ok()).collect();
    let resident = pages.get(1).copied().unwrap_or(0) * PAGE;
    let (read, write) = io(pid).unwrap_or_default();
    // Linux has no Windows "private bytes" counter. PSS is the honest number for memory
    // pressure: it counts each shared page once, proportional to its sharers, where RSS counts
    // shared code and libraries once per client. The dashboard and monitor use `private`.
    let private = pss(pid).unwrap_or(resident);
    Some(Reading { cpu, working_set: resident, private, read, write })
}

/// Proportional set size from `smaps_rollup`, in bytes.
fn pss(pid: u32) -> Option<u64> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?;
    text.lines().find_map(|l| l.strip_prefix("Pss:"))?.split_whitespace().next()?.parse::<u64>().ok().map(|kb| kb * 1024)
}

fn io(pid: u32) -> Option<(u64, u64)> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/io")).ok()?;
    let mut read = 0;
    let mut write = 0;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("read_bytes:") {
            read = v.trim().parse().ok()?;
        } else if let Some(v) = line.strip_prefix("write_bytes:") {
            write = v.trim().parse().ok()?;
        }
    }
    Some((read, write))
}

pub fn mem_strikes(prev: u32, private_mb: u64, limit_mb: u32) -> u32 {
    if limit_mb > 0 && private_mb > u64::from(limit_mb) { prev.saturating_add(1) } else { 0 }
}

pub fn mem_throttled(prev: bool, private_mb: u64, limit_mb: u32) -> bool {
    let limit = u64::from(limit_mb);
    match limit_mb {
        0 => false,
        _ if private_mb > limit => true,
        _ if private_mb * 10 < limit * 9 => false,
        _ => prev,
    }
}

pub const MEM_STRIKES: u32 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_this_process() {
        let pid = std::process::id();
        let t0 = start_time(pid).unwrap();
        let r = read(pid, t0).unwrap();
        assert!(r.working_set > 0);
        assert!(r.private > 0);
        assert!(read(pid, t0 + 1).is_none());
    }

    #[test]
    fn strikes_and_throttle_have_hysteresis() {
        assert_eq!(mem_strikes(2, 200, 100), 3);
        assert_eq!(mem_strikes(2, 90, 100), 0);
        assert_eq!(mem_strikes(2, 999, 0), 0);
        assert!(mem_throttled(false, 101, 100));
        assert!(mem_throttled(true, 95, 100));
        assert!(!mem_throttled(true, 89, 100));
        assert!(!mem_throttled(true, 1000, 0));
    }
}
