//! GPU use per process, from the same Windows performance counters Task Manager shows: the busiest
//! engine's utilization and dedicated GPU memory. English counter names work on any Windows language. The
//! counters don't touch the clients. One query is kept open; utilization is a rate, so it has values
//! from the second collection on. When Windows can't say, this reports nothing (logged once).

use std::collections::HashMap;

use windows::Win32::System::Performance::{
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
    PdhGetFormattedCounterArrayW, PdhOpenQueryW,
};
use windows::core::{PCWSTR, w};

const OK: u32 = 0;
const MB: f64 = 1024.0 * 1024.0;

/// The process id in a counter instance name such as `pid_1234_luid_0x0_0x1_phys_0_eng_0_engtype_3D` (pure).
fn pid_of(instance: &str) -> Option<u32> {
    instance.strip_prefix("pid_")?.split('_').next()?.parse().ok()
}

/// The values of the instances, summed per process (pure).
fn by_pid<'a>(items: impl Iterator<Item = (&'a str, f64)>) -> HashMap<u32, f64> {
    let mut out = HashMap::new();
    for (name, value) in items {
        if let Some(pid) = pid_of(name) {
            *out.entry(pid).or_insert(0.0) += value;
        }
    }
    out
}

/// Each process's busiest engine of any type, Microsoft's stated Task Manager rule (pure). An
/// engine can be listed more than once under one name: those add up first.
fn busiest_engine<'a>(items: impl Iterator<Item = (&'a str, f64)>) -> HashMap<u32, f64> {
    let mut engines = HashMap::new();
    for (name, value) in items {
        *engines.entry(name).or_insert(0.0) += value;
    }
    let mut out = HashMap::new();
    for (name, value) in engines {
        if let Some(pid) = pid_of(name) {
            let busiest: &mut f64 = out.entry(pid).or_insert(0.0);
            *busiest = busiest.max(value);
        }
    }
    out
}

/// Each process's GPU use now; a map is None while its counter has no values.
pub struct Usage {
    /// The busiest engine's utilization, percent.
    pub pct: Option<HashMap<u32, f64>>,
    /// Dedicated GPU memory, MB.
    pub mem_mb: Option<HashMap<u32, f64>>,
}

pub struct Gpu {
    query: PDH_HQUERY,
    engine: PDH_HCOUNTER,
    memory: PDH_HCOUNTER,
}

// SAFETY: PDH handles aren't tied to a thread; `Sampler` keeps this behind a mutex.
unsafe impl Send for Gpu {}

impl Drop for Gpu {
    fn drop(&mut self) {
        // SAFETY: the query is ours; closing it closes its counters.
        unsafe { PdhCloseQuery(self.query) };
    }
}

impl Gpu {
    /// None when Windows has no such counters.
    pub fn open() -> Option<Gpu> {
        let (mut query, mut engine, mut memory) = (PDH_HQUERY::default(), PDH_HCOUNTER::default(), PDH_HCOUNTER::default());
        // SAFETY: the out-pointers are locals; the paths are NUL-terminated literals.
        let status = unsafe {
            let mut s = PdhOpenQueryW(PCWSTR::null(), 0, &mut query);
            if s == OK {
                s = PdhAddEnglishCounterW(query, w!(r"\GPU Engine(*)\Utilization Percentage"), 0, &mut engine);
            }
            if s == OK {
                s = PdhAddEnglishCounterW(query, w!(r"\GPU Process Memory(*)\Dedicated Usage"), 0, &mut memory);
            }
            s
        };
        if status != OK {
            log::info!("GPU use per client isn't available here (PDH {status:#x})");
            if !query.is_invalid() {
                // SAFETY: opened above.
                unsafe { PdhCloseQuery(query) };
            }
            return None;
        }
        Some(Gpu { query, engine, memory })
    }

    pub fn read(&self) -> Usage {
        // SAFETY: the query is ours and open.
        let collected = unsafe { PdhCollectQueryData(self.query) };
        if collected != OK {
            log::debug!("GPU counters not collected (PDH {collected:#x})");
            return Usage { pct: None, mem_mb: None };
        }
        Usage {
            pct: values(self.engine, |items| busiest_engine(items)),
            mem_mb: values(self.memory, |items| by_pid(items.map(|(n, v)| (n, v / MB)))),
        }
    }
}

/// The graphics card with the most dedicated video memory, and that memory in MB; software
/// renderers left out. None when Windows lists none.
pub fn adapter() -> Option<(String, f64)> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};
    // SAFETY: plain COM calls on interfaces this function owns; the loop ends at the first index Windows refuses.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().inspect_err(|e| log::debug!("no DXGI factory: {e}")).ok()?;
        (0..)
            .map_while(|i| factory.EnumAdapters1(i).ok())
            .filter_map(|a| a.GetDesc1().ok())
            .filter(|d| d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0)
            .max_by_key(|d| d.DedicatedVideoMemory)
            .map(|d| {
                let len = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
                (String::from_utf16_lossy(&d.Description[..len]).trim().to_owned(), d.DedicatedVideoMemory as f64 / MB)
            })
    }
}

/// The counter's current instances, handed to `f` as (name, value).
fn values<T>(counter: PDH_HCOUNTER, f: impl FnOnce(&mut dyn Iterator<Item = (&str, f64)>) -> T) -> Option<T> {
    let (mut bytes, mut count) = (0u32, 0u32);
    // SAFETY: asks for the size only: no buffer is passed.
    let asked = unsafe { PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut bytes, &mut count, None) };
    if asked != PDH_MORE_DATA {
        log::debug!("GPU counter has no values yet (PDH {asked:#x})");
        return None;
    }
    // u64s: the buffer holds items with pointers and doubles, then their names.
    let mut buf = vec![0u64; (bytes as usize).div_ceil(8)];
    let items = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
    // SAFETY: `buf` has the `bytes` the call asked for and is aligned for its items.
    let got = unsafe { PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut bytes, &mut count, Some(items)) };
    if got != OK {
        log::debug!("GPU counter not read (PDH {got:#x})");
        return None;
    }
    // SAFETY: the call wrote `count` items into `buf`; their name pointers stay inside it.
    let named: Vec<(String, f64)> = unsafe {
        std::slice::from_raw_parts(items, count as usize)
            .iter()
            .filter(|i| i.FmtValue.CStatus <= 1)
            .filter_map(|i| Some((i.szName.to_string().ok()?, i.FmtValue.Anonymous.doubleValue)))
            .collect()
    };
    // Empty is a first read with nothing to compare yet, not every process using 0.
    if named.is_empty() {
        return None;
    }
    Some(f(&mut named.iter().map(|(n, v)| (n.as_str(), *v))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_a_process_over_its_instances() {
        let items = [
            ("pid_10_luid_0x0_0x1_phys_0", 5.0),
            ("pid_10_luid_0x0_0x2_phys_0", 2.0),
            ("pid_11_luid_0x0_0x1_phys_0", 1.0),
            ("not_a_process", 9.0),
        ];
        let m = by_pid(items.into_iter());
        assert_eq!(m.get(&10), Some(&7.0));
        assert_eq!(m.get(&11), Some(&1.0));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn a_process_is_as_busy_as_its_busiest_engine() {
        let items = [
            ("pid_10_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 30.0),
            ("pid_10_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 15.0),
            ("pid_10_luid_0x0_0x1_phys_0_eng_1_engtype_3D", 40.0),
            ("pid_10_luid_0x0_0x1_phys_0_eng_2_engtype_Copy", 10.0),
            ("pid_11_luid_0x0_0x1_phys_0_eng_3_engtype_VideoDecode", 60.0),
            ("pid_11_luid_0x0_0x1_phys_0_eng_0_engtype_3D", 5.0),
            ("not_a_process", 99.0),
        ];
        let m = busiest_engine(items.into_iter());
        assert_eq!(m.get(&10), Some(&45.0), "a repeated engine adds up; engines don't");
        assert_eq!(m.get(&11), Some(&60.0), "any engine type counts");
        assert_eq!(m.len(), 2);
    }

    /// The real graphics card: `cargo test -- --ignored --nocapture finds_the_real_adapter`.
    #[test]
    #[ignore]
    fn finds_the_real_adapter() {
        let (name, vram) = adapter().expect("an adapter");
        println!("GPU {name}: {vram:.0} MB");
        assert!(!name.is_empty());
    }

    /// Real counters (needs a desktop session): `cargo test -- --ignored reads_real_counters`.
    #[test]
    #[ignore]
    fn reads_real_counters() {
        let g = Gpu::open().expect("counters");
        g.read();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let u = g.read();
        let mem = u.mem_mb.expect("memory values");
        let pct = u.pct.expect("engine values");
        println!("GPU processes: {} with memory, {} with engine use", mem.len(), pct.len());
        for (pid, mb) in mem.iter().take(5) {
            println!("GPUMEM pid {pid}: {mb:.0} MB, busiest engine {:.1}%", pct.get(pid).copied().unwrap_or(0.0));
        }
        assert!(!mem.is_empty());
    }
}
