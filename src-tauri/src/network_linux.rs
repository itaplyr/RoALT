//! Network guard on Linux: there is no Network List Manager equivalent. RoALT treats every
//! connection as private and never locks the vault or refuses a launch for the network.

use std::sync::Mutex;

use tauri::AppHandle;

use crate::App;
use crate::model::NetworkStatus;

pub struct Network(Mutex<NetworkStatus>);

impl Network {
    pub fn new() -> Self {
        Self(Mutex::new(scan()))
    }

    pub fn get(&self) -> NetworkStatus {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

pub fn scan() -> NetworkStatus {
    NetworkStatus { public: false }
}

pub fn blocked(_s: &App) -> Option<String> {
    None
}

pub fn spawn(_app: AppHandle) {}
