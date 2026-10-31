//! What each client uses: CPU from `GetProcessTimes` deltas, memory from
//! `K32GetProcessMemoryInfo`, IO from `GetProcessIoCounters`, GPU from `gpu`. Each sample opens a short-lived
//! `PROCESS_QUERY_LIMITED_INFORMATION` handle checked against the client's start time; nothing
//! reads the client's memory (no VM_READ).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{FILETIME, HANDLE};
use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX, PROCESS_MEMORY_COUNTERS_EX2};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{
    GetProcessIoCounters, GetProcessTimes, GetSystemTimes, IO_COUNTERS, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};

use super::gpu::{Gpu, Usage};
use super::process::{open, started};
use crate::model::{ClientMetrics, Limits, SystemUsage};

/// Samples are reused for this long, so several views polling don't add up. A little under the
/// views' 2 s, so a poll arriving a hair early gets a new sample instead of waiting for the next.
const CACHE: Duration = Duration::from_millis(1900);
const MB: f64 = 1024.0 * 1024.0;

/// How long a sample that took `read` is reused: many clients make the counters slow, and this
/// keeps them from reading all the time.
fn ttl(read: Duration) -> Duration {
    CACHE.max(read * 3)
}

/// One reading of a process.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    /// Kernel + user time, 100 ns ticks.
    pub cpu: u64,
    /// Private working set: Task Manager's Memory column, what a trim lowers.
    pub memory: u64,
    /// Private bytes: Task Manager's Commit size, what a leak grows and the memory limits watch.
    pub private: u64,
    pub read: u64,
    pub write: u64,
}

/// The previous reading of each client, for the deltas.
struct Last {
    at: Instant,
    t0: u64,
    r: Reading,
}

/// The PC's last sample: when, what, and its CPU times (idle, kernel + user).
type SystemSample = (Instant, SystemUsage, (u64, u64));

pub struct Sampler {
    last: Mutex<HashMap<String, Last>>,
    /// The last sample and when it stops being reused.
    cache: Mutex<Option<(Instant, Vec<ClientMetrics>)>>,
    /// Opened on the first sample; stays None where Windows has no such counters.
    gpu: Mutex<Option<Option<Gpu>>>,
    /// For `system`.
    system: Mutex<Option<SystemSample>>,
}

impl Sampler {
    pub fn new() -> Sampler {
        Sampler { last: Mutex::new(HashMap::new()), cache: Mutex::new(None), gpu: Mutex::new(None), system: Mutex::new(None) }
    }

    /// How busy the whole PC is, reused for `CACHE`; None if Windows doesn't say.
    pub fn system(&self) -> Option<SystemUsage> {
        let mut last = self.system.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, usage, _)) = last.as_ref()
            && at.elapsed() < CACHE
        {
            return Some(*usage);
        }
        let times = cpu_times()?;
        let (used, total) = memory()?;
        let usage = SystemUsage { cpu_pct: last.as_ref().and_then(|(_, _, before)| busy_pct(*before, times)), mem_used_mb: used as f64 / MB, mem_total_mb: total as f64 / MB };
        *last = Some((Instant::now(), usage, times));
        Some(usage)
    }

    /// Metrics of the running clients (id, pid, start time, limits as `Tracker::clients`);
    /// a client whose process can't be read is left out.
    pub fn sample(&self, clients: &[(String, u32, u64, Limits)]) -> Vec<ClientMetrics> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((until, m)) = cache.as_ref()
            && Instant::now() < *until
            && m.len() == clients.len()
            && m.iter().zip(clients).all(|(m, c)| m.instance_id == c.0 && m.pid == c.1)
        {
            return m.clone();
        }
        let cores = std::thread::available_parallelism()
            .inspect_err(|e| log::debug!("metrics: core count unknown, using 1: {e}"))
            .map_or(1, |n| n.get()) as f64;
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        last.retain(|id, _| clients.iter().any(|c| c.0 == *id));
        let mut gpu = self.gpu.lock().unwrap_or_else(|e| e.into_inner());
        let gpu = gpu.get_or_insert_with(Gpu::open);
        // After opening the GPU counters, which can take seconds once: the cache lasts from here.
        // Not from the end of the read: a poll every 2 s would then keep finding it still fresh.
        let began = Instant::now();
        let usage = match gpu {
            Some(g) => g.read(),
            None => Usage { pct: None, mem_mb: None },
        };
        // What the rates are measured from: when these values were read.
        let now = Instant::now();
        // A process using nothing has no counter instance: 0, when the counters answered at all.
        let of = |m: &Option<HashMap<u32, f64>>, pid: u32| m.as_ref().map(|m| m.get(&pid).copied().unwrap_or(0.0));
        let mut out = Vec::new();
        for (id, pid, t0, _) in clients {
            let Some(r) = read(*pid, *t0) else { continue };
            let prev = last.get(id).filter(|l| l.t0 == *t0);
            let secs = prev.map(|l| now.duration_since(l.at).as_secs_f64()).filter(|&s| s > 0.0);
            let rate = |f: fn(&Reading) -> u64| prev.zip(secs).map(|(l, s)| f(&r).saturating_sub(f(&l.r)) as f64 / s);
            out.push(ClientMetrics {
                instance_id: id.clone(),
                pid: *pid,
                // 100 ns ticks per second of one core: 1e7.
                cpu_pct: rate(|r| r.cpu).map(|t| (t / 1e7 / cores * 100.0).min(100.0)),
                memory_mb: r.memory as f64 / MB,
                private_mb: r.private as f64 / MB,
                read_bps: rate(|r| r.read),
                write_bps: rate(|r| r.write),
                gpu_pct: of(&usage.pct, *pid).map(|p| p.min(100.0)),
                gpu_mem_mb: of(&usage.mem_mb, *pid),
            });
            last.insert(id.clone(), Last { at: now, t0: *t0, r });
        }
        let read = began.elapsed();
        log::debug!("metrics: {} clients read in {read:?}", clients.len());
        *cache = Some((began + ttl(read), out.clone()));
        out
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

/// All cores' CPU time so far, 100 ns ticks: idle, and kernel (which includes idle) + user.
fn cpu_times() -> Option<(u64, u64)> {
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: three local FILETIMEs to write to.
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }
        .inspect_err(|e| log::debug!("metrics: system times unreadable: {e}"))
        .ok()?;
    Some((ticks(idle), ticks(kernel) + ticks(user)))
}

/// Physical memory in use and in all, bytes.
fn memory() -> Option<(u64, u64)> {
    let mut m = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    // SAFETY: a local MEMORYSTATUSEX whose dwLength says its size.
    unsafe { GlobalMemoryStatusEx(&mut m) }
        .inspect_err(|e| log::debug!("metrics: memory status unreadable: {e}"))
        .ok()?;
    Some((m.ullTotalPhys.saturating_sub(m.ullAvailPhys), m.ullTotalPhys))
}

/// The share of time not idle between two `cpu_times` (pure); None without any time passing.
fn busy_pct(before: (u64, u64), now: (u64, u64)) -> Option<f64> {
    let total = now.1.checked_sub(before.1).filter(|&t| t > 0)?;
    let idle = now.0.saturating_sub(before.0).min(total);
    Some((total - idle) as f64 / total as f64 * 100.0)
}

/// One reading of `pid`, only while it is still the process started at `t0`.
pub fn read(pid: u32, t0: u64) -> Option<Reading> {
    let h = open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE)?;
    if started(*h) != Some(t0) {
        return None;
    }
    let cpu = cpu_ticks(*h)?;
    let (ex2, ex) = (size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32, size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32);
    let mut mem = PROCESS_MEMORY_COUNTERS_EX2 { cb: ex2, ..Default::default() };
    let counters = (&raw mut mem).cast::<PROCESS_MEMORY_COUNTERS>();
    let mut io = IO_COUNTERS::default();
    // SAFETY: `h` is live with PROCESS_QUERY_LIMITED_INFORMATION (all these need); `mem` is a
    // local PROCESS_MEMORY_COUNTERS_EX2, which is as long as the sizes given and starts with the
    // shorter forms; `io` is a local.
    unsafe {
        // Older Windows only knows the shorter form.
        if !K32GetProcessMemoryInfo(*h, counters, ex2).as_bool() {
            K32GetProcessMemoryInfo(*h, counters, ex)
                .ok()
                .inspect_err(|e| log::debug!("metrics: memory of process {pid} unreadable: {e}"))
                .ok()?;
        }
        GetProcessIoCounters(*h, &mut io).inspect_err(|e| log::debug!("metrics: I/O of process {pid} unreadable: {e}")).ok()?;
    }
    Some(Reading {
        cpu,
        // Zero on that older Windows, which can only say the whole working set.
        memory: if mem.PrivateWorkingSetSize > 0 { mem.PrivateWorkingSetSize } else { mem.WorkingSetSize } as u64,
        private: mem.PrivateUsage as u64,
        read: io.ReadTransferCount,
        write: io.WriteTransferCount,
    })
}

fn cpu_ticks(h: HANDLE) -> Option<u64> {
    let (mut created, mut exited, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: `h` is live with PROCESS_QUERY_LIMITED_INFORMATION; the out pointers are locals.
    unsafe { GetProcessTimes(h, &mut created, &mut exited, &mut kernel, &mut user) }
        .inspect_err(|e| log::debug!("metrics: CPU time unreadable: {e}"))
        .ok()?;
    Some(ticks(kernel) + ticks(user))
}

fn ticks(f: FILETIME) -> u64 {
    (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
}

/// How many 10 s memory samples in a row a client has been over the limit, after one more
/// sample of `private_mb` (pure). 0 when the limit is off.
pub fn mem_strikes(prev: u32, private_mb: u64, limit_mb: u32) -> u32 {
    if limit_mb > 0 && private_mb > u64::from(limit_mb) { prev.saturating_add(1) } else { 0 }
}

/// Whether a client is held by the memory throttle after a sample of `private_mb` (pure): on
/// over `limit_mb`, off again under 90 % of it, else as it was (`prev`). Never with 0 (off).
pub fn mem_throttled(prev: bool, private_mb: u64, limit_mb: u32) -> bool {
    let limit = u64::from(limit_mb);
    match limit_mb {
        0 => false,
        _ if private_mb > limit => true,
        _ if private_mb * 10 < limit * 9 => false,
        _ => prev,
    }
}

/// Samples over the limit in a row that restart a client.
pub const MEM_STRIKES: u32 = 3;

#[cfg(test)]
mod tests {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    use super::*;
    use crate::instances::process::start_time;

    #[test]
    fn reads_a_running_process() {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        let mut child = Command::new(std::path::Path::new(&root).join(r"System32\PING.EXE"))
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
            .unwrap();
        let (pid, t0) = (child.id(), start_time(child.id()).unwrap());
        let r = read(pid, t0).unwrap();
        assert!(r.memory > 0 && r.private > 0, "{r:?}");
        assert_eq!(read(pid, t0 - 10), None, "a reused pid is not the client");

        let s = Sampler::new();
        let clients = [("i".to_owned(), pid, t0, Limits::default())];
        let first = s.sample(&clients);
        assert_eq!((first.len(), first[0].cpu_pct, first[0].read_bps), (1, None, None), "no delta yet");
        assert_eq!(s.sample(&clients), first, "cached for 2 s");
        *s.cache.lock().unwrap() = None;
        std::thread::sleep(Duration::from_millis(50));
        let second = s.sample(&clients);
        assert!(second[0].cpu_pct.is_some_and(|c| (0.0..=100.0).contains(&c)), "{second:?}");
        assert!(second[0].memory_mb > 0.0);
        child.kill().unwrap();
        child.wait().unwrap();
        *s.cache.lock().unwrap() = None;
        assert!(s.sample(&clients).is_empty(), "gone");
    }

    #[test]
    fn a_slow_read_is_reused_for_longer() {
        assert_eq!(ttl(Duration::from_millis(10)), CACHE);
        assert_eq!(ttl(Duration::from_secs(2)), Duration::from_secs(6));
    }

    #[test]
    fn the_pc_is_as_busy_as_its_cores_are_not_idle() {
        assert_eq!(busy_pct((100, 1_000), (150, 1_200)), Some(75.0));
        assert_eq!(busy_pct((100, 1_000), (300, 1_200)), Some(0.0));
        assert_eq!(busy_pct((100, 1_000), (100, 1_000)), None, "no time passed");
        assert_eq!(busy_pct((100, 1_000), (100, 900)), None, "a clock can't run back");
        let s = Sampler::new();
        let first = s.system().expect("Windows says how busy it is");
        assert!(first.mem_total_mb > first.mem_used_mb && first.mem_used_mb > 0.0, "{first:?}");
        assert_eq!(first.cpu_pct, None, "nothing to compare with yet");
        assert_eq!(s.system(), Some(first), "cached");
    }

    #[test]
    fn memory_strikes() {
        assert_eq!(mem_strikes(0, 5_000, 4_096), 1);
        assert_eq!(mem_strikes(2, 5_000, 4_096), MEM_STRIKES);
        assert_eq!(mem_strikes(2, 4_096, 4_096), 0, "at the limit is not over it");
        assert_eq!(mem_strikes(2, 99_999, 0), 0, "off");
        assert_eq!(mem_strikes(u32::MAX, 5_000, 1), u32::MAX);
    }

    #[test]
    fn memory_throttle_has_hysteresis() {
        assert!(mem_throttled(false, 4_097, 4_096), "over");
        assert!(!mem_throttled(false, 4_096, 4_096), "at the limit is not over it");
        assert!(mem_throttled(true, 3_700, 4_096), "between 90 % and the limit: stays");
        assert!(!mem_throttled(false, 3_700, 4_096), "between: stays off");
        assert!(!mem_throttled(true, 3_686, 4_096), "under 90 %: lifted");
        assert!(!mem_throttled(true, 99_999, 0), "off");
    }
}
