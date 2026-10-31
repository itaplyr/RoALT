//! Multiple clients on Linux are provided by Cordial profiles (or Sober, which has no
//! multi-instance support). There is no Windows singleton mutex to hold; RoALT only records
//! whether the user asked for it so the rest of the app behaves consistently.
#![allow(dead_code)]


use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::error::Result;
use crate::model::MultiInstanceStatus;

pub const SINGLETON_NAME: &str = "ROBLOX_singletonEvent";

pub struct Multi {
    active: Mutex<bool>,
    launches: AtomicUsize,
}

pub struct Launching<'a>(&'a Multi);

impl Drop for Launching<'_> {
    fn drop(&mut self) {
        self.0.launches.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Multi {
    pub fn new() -> Multi {
        Multi { active: Mutex::new(false), launches: AtomicUsize::new(0) }
    }

    pub fn enable(&self) -> Result<()> {
        *self.active.lock().unwrap_or_else(|e| e.into_inner()) = true;
        Ok(())
    }

    pub fn take_over(&self) -> Result<()> {
        self.enable()
    }

    pub fn disable(&self) {
        *self.active.lock().unwrap_or_else(|e| e.into_inner()) = false;
    }

    pub fn is_active(&self) -> bool {
        *self.active.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn lock_shared(&self) {}

    pub fn release_shared(&self) {}

    pub fn launching(&self, _protect: bool) -> Launching<'_> {
        self.launches.fetch_add(1, Ordering::SeqCst);
        Launching(self)
    }

    pub fn release_if(&self, idle: impl FnOnce() -> bool) {
        if idle() {
            self.disable();
        }
    }

    pub fn in_flight(&self) -> bool {
        self.launches.load(Ordering::SeqCst) > 0
    }

    pub fn adopt(&self, _clients: &[(u32, u64)]) {}

    pub fn has_survivors(&self) -> bool {
        false
    }

    pub fn status(&self, enabled: bool, protect_shared_login: bool) -> MultiInstanceStatus {
        MultiInstanceStatus {
            enabled,
            active: enabled && self.is_active(),
            protect_shared_login,
            blocked_by: Vec::new(),
            message: None,
        }
    }
}

impl Default for Multi {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabling_is_reflected_in_status() {
        let m = Multi::new();
        assert!(!m.is_active());
        m.enable().unwrap();
        assert!(m.status(true, true).active);
        m.disable();
        assert!(!m.is_active());
    }
}
