//! RoALT's own updates on Linux are left to the package manager. The settings and status
//! commands still work, but automatic install is refused with a clear message.
#![allow(dead_code)]


use std::sync::atomic::{AtomicBool, Ordering};

use tauri::AppHandle;

use crate::error::{AppError, Result};
use crate::model::AppUpdate;

pub const UPDATE_EVENT: &str = "app://update";

#[derive(Default)]
pub struct SelfUpdater {
    installing: AtomicBool,
}

impl SelfUpdater {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn installing(&self) -> bool {
        self.installing.load(Ordering::SeqCst)
    }
}

pub fn status(_s: &crate::App) -> AppUpdate {
    AppUpdate { current: env!("CARGO_PKG_VERSION").into(), available: None }
}

pub async fn check(_app: &AppHandle) -> Result<AppUpdate> {
    Ok(AppUpdate { current: env!("CARGO_PKG_VERSION").into(), available: None })
}

pub fn switched(_app: &AppHandle, _on: bool) {}

pub fn spawn(_app: AppHandle) {}

pub async fn install(_app: &AppHandle) -> Result<()> {
    Err(AppError::Invalid(tr!(
        "RoALT updates are handled by your Linux package or build. Update it the way you installed it."
    )))
}
