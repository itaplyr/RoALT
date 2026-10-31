//! Per-client limits on Linux, through the scheduler rather than a container: CPU affinity
//! (`sched_setaffinity`), niceness (a soft CPU cap) and efficiency priority (`eco`). Linux has no
//! per-process working-set trim equivalent, so `trim` is a no-op. The planning functions are the
//! same as Windows, so auto-spread, the adaptive defaults and the memory throttle keep working.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::Result;
use crate::model::{Limits, Settings};

/// What has been applied to one instance's current process, so the tracker's once-a-second
/// sync does not repeat syscalls.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Applied {
    pid: u32,
    t0: u64,
    limits: Limits,
}

pub struct Limiter {
    applied: Mutex<HashMap<String, Applied>>,
}

impl Limiter {
    pub fn new() -> Limiter {
        Limiter { applied: Mutex::new(HashMap::new()) }
    }

    pub fn apply(&self, id: &str, pid: u32, t0: u64, limits: Limits) -> Result<()> {
        let mut map = self.applied.lock().unwrap_or_else(|e| e.into_inner());
        if map.get(id).is_some_and(|a| *a == Applied { pid, t0, limits }) {
            return Ok(());
        }
        let nice = nice_for(limits);
        set_nice(pid, nice);
        if let Some(mask) = limits.affinity {
            set_affinity(pid, mask);
        } else {
            clear_affinity(pid);
        }
        map.insert(id.to_owned(), Applied { pid, t0, limits });
        Ok(())
    }

    pub fn release_all(&self) {
        let applied = std::mem::take(&mut *self.applied.lock().unwrap_or_else(|e| e.into_inner()));
        for (_, a) in applied {
            set_nice(a.pid, 0);
            clear_affinity(a.pid);
        }
    }

    pub fn sync(&self, clients: &[(String, u32, u64, Limits)]) {
        self.applied.lock().unwrap_or_else(|e| e.into_inner()).retain(|id, _| clients.iter().any(|c| c.0 == *id));
        for (id, pid, t0, limits) in clients {
            if let Err(e) = self.apply(id, *pid, *t0, *limits) {
                log::warn!("could not apply limits to instance {id}: {e}");
            }
        }
    }
}

/// A soft CPU cap, mapped to niceness. `eco` always yields to normal-priority work.
fn nice_for(l: Limits) -> i32 {
    let cap = l.cpu_cap.unwrap_or(100);
    let mut nice = match cap {
        100..=255 => 0,
        75..=99 => 4,
        50..=74 => 8,
        25..=49 => 12,
        _ => 16,
    };
    if l.eco {
        nice = nice.max(15);
    }
    nice
}

fn set_nice(pid: u32, nice: i32) {
    // SAFETY: setpriority on another process only changes its scheduling priority.
    let r = unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, nice) };
    if r != 0 {
        log::debug!("could not set niceness of process {pid}: {}", std::io::Error::last_os_error());
    }
}

fn set_affinity(pid: u32, mask: u64) {
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    // SAFETY: `set` is a local cpu_set_t of the size the CPU_* macros expect.
    unsafe { libc::CPU_ZERO(&mut set) };
    for core in 0..64 {
        if mask & (1 << core) != 0 {
            // SAFETY: as above; CPU_SET bounds-checks against CPU_SETSIZE.
            unsafe { libc::CPU_SET(core, &mut set) };
        }
    }
    // SAFETY: the pid is only being retargeted; `set` is live for the call.
    let r = unsafe { libc::sched_setaffinity(pid as libc::pid_t, std::mem::size_of::<libc::cpu_set_t>(), &set) };
    if r != 0 {
        log::debug!("could not set affinity of process {pid}: {}", std::io::Error::last_os_error());
    }
}

fn clear_affinity(pid: u32) {
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    // SAFETY: a local cpu_set_t, filled for every CPU this process may use.
    unsafe {
        libc::CPU_ZERO(&mut set);
        let n = libc::sysconf(libc::_SC_NPROCESSORS_ONLN);
        let n = if n > 0 { n as usize } else { 1 };
        for core in 0..n.min(libc::CPU_SETSIZE as usize) {
            libc::CPU_SET(core, &mut set);
        }
        let _ = libc::sched_setaffinity(pid as libc::pid_t, std::mem::size_of::<libc::cpu_set_t>(), &set);
    }
}

pub fn plan(
    clients: &[(String, u32, u64, Limits)],
    v: &Settings,
    foreground: Option<u32>,
    cores: u32,
    throttled: &[String],
) -> Vec<(String, u32, u64, Limits)> {
    let n = clients.len() as u32;
    clients
        .iter()
        .enumerate()
        .map(|(i, (id, pid, t0, manual))| {
            let share = (v.auto_spread && n > 1).then(|| spread(i as u32, n, cores));
            let mut l = effective(*manual, foreground == Some(*pid), v, share);
            if v.mem_throttle.mb > 0 && throttled.contains(id) {
                l.cpu_cap = Some(throttle_cap(l.cpu_cap, v.mem_throttle.cpu_cap));
            }
            (id.clone(), *pid, *t0, l)
        })
        .collect()
}

pub fn effective(manual: Limits, foreground: bool, v: &Settings, share: Option<u64>) -> Limits {
    let mut l = if manual == Limits::default() && v.adaptive.enabled && !foreground { v.adaptive.background } else { manual };
    if l.trim_every_min.is_none() && v.trim_every_min > 0 {
        l.trim_every_min = Some(v.trim_every_min);
    }
    if l.affinity.is_none() {
        l.affinity = share;
    }
    l
}

pub fn throttle_cap(own: Option<u8>, throttle: u8) -> u8 {
    own.map_or(throttle, |c| c.min(throttle))
}

pub fn spread(i: u32, n: u32, cores: u32) -> u64 {
    let cores = cores.clamp(1, 64);
    if n >= cores {
        return 1 << (i % cores);
    }
    (0..cores).filter(|c| c % n == i % n).fold(0, |m, c| m | 1 << c)
}

pub fn clamp_affinity(mask: u64, system: u64) -> Option<u64> {
    Some(mask & system).filter(|&m| m != 0)
}

pub fn trim(_pid: u32, _t0: u64) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreads_cores_and_clamps() {
        assert_eq!(spread(0, 2, 4), 0b0101);
        assert_eq!(spread(1, 2, 4), 0b1010);
        assert_eq!(spread(2, 3, 2), 1);
        assert_eq!(clamp_affinity(0b111, 0b101), Some(0b101));
        assert_eq!(clamp_affinity(0b010, 0b101), None);
    }

    #[test]
    fn manual_limits_win_and_throttle_tightens() {
        let v = Settings::default();
        let manual = Limits { cpu_cap: Some(40), ..Default::default() };
        assert_eq!(effective(manual, true, &v, Some(3)).cpu_cap, Some(40));
        assert_eq!(throttle_cap(Some(40), 25), 25);
        assert_eq!(throttle_cap(Some(10), 25), 10);
    }

    #[test]
    fn niceness_follows_the_cap_and_eco() {
        assert_eq!(nice_for(Limits::default()), 0);
        assert_eq!(nice_for(Limits { cpu_cap: Some(25), ..Default::default() }), 12);
        assert_eq!(nice_for(Limits { eco: true, ..Default::default() }), 15);
    }
}
