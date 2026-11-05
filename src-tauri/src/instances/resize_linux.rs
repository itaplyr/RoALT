//! Free window resizing is a Windows-only feature (low-level mouse hooks and WIN32 sizing
//! loops). Linux uses the compositor's own resize; the tracker calls these hooks regardless, so
//! they stay as no-ops here.

pub fn start() {}

pub fn set_clients(_pids: Vec<u32>) {}

#[allow(dead_code)]
pub fn unhide_all(_limit: std::time::Duration) {}
