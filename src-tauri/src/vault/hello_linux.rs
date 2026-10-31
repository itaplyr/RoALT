//! Linux has no Windows Hello equivalent wired up; re-authentication uses the master password.

use crate::error::{AppError, Result};

pub async fn windows_hello(_window: &tauri::WebviewWindow, _message: &str) -> Result<()> {
    Err(AppError::Invalid(tr!("Set a master password in Settings to protect cookies on this Linux desktop.")))
}

pub async fn hello_available() -> bool {
    false
}
