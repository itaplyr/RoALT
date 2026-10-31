//! Roblox clients RoALT started: the multi-instance lock, process tracking, log watching
//! and closing. Nothing here reads or writes another process's memory or handles; anti-AFK
//! (`afk`) presses a key the way a keyboard does.

#[cfg(windows)]
pub mod afk;
#[cfg(not(windows))]
#[path = "afk_linux.rs"]
pub mod afk;

#[cfg(windows)]
mod capture;
#[cfg(windows)]
mod gpu;

#[cfg(windows)]
pub mod limits;
#[cfg(not(windows))]
#[path = "limits_linux.rs"]
pub mod limits;

pub mod logwatch;

#[cfg(windows)]
pub mod metrics;
#[cfg(not(windows))]
#[path = "metrics_linux.rs"]
pub mod metrics;

#[cfg(windows)]
pub mod multi;
#[cfg(not(windows))]
#[path = "multi_linux.rs"]
pub mod multi;

#[cfg(windows)]
pub mod process;
#[cfg(not(windows))]
#[path = "process_linux.rs"]
pub mod process;

#[cfg(windows)]
pub mod qos;
#[cfg(not(windows))]
#[path = "qos_linux.rs"]
pub mod qos;

#[cfg(windows)]
pub mod resize;
#[cfg(not(windows))]
#[path = "resize_linux.rs"]
pub mod resize;

#[cfg(windows)]
pub mod window;
#[cfg(not(windows))]
#[path = "window_linux.rs"]
pub mod window;

mod tracker;

pub mod rejoin;
pub use tracker::{Tracker, Transition, challenged, closed, hung, unexpected, wrong_account};
