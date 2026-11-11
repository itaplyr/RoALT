//! "Mute clients on launch": sets one client's own Windows audio session muted or not (Core Audio,
//! by process id); Roblox's settings file is untouched. Windows may remember a mute for the next
//! session of the same exe, so every RoALT client gets set, unmuted when the setting is off; a
//! Roblox started outside RoALT can still inherit one. The user can change it in the volume mixer.

use std::time::Duration;

use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume,
    MMDeviceEnumerator, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::core::{Interface, Result};

/// A client can open its audio session a moment after it reaches a game: tries this often.
const TRIES: u32 = 5;
const STEP: Duration = Duration::from_secs(2);

/// Mutes (or unmutes) every audio session of process `pid` on every active output device.
/// Blocking (it retries while the client has none yet); true when one was set. Failures are logged.
pub fn set_mute(pid: u32, mute: bool) -> bool {
    for attempt in 1..=TRIES {
        match set_once(pid, mute) {
            Ok(n) if n > 0 => return true,
            Ok(_) => {}
            Err(e) => log::debug!("client {pid}: setting muted: {mute} failed (try {attempt}): {e}"),
        }
        if attempt < TRIES {
            std::thread::sleep(STEP);
        }
    }
    false
}

/// How many of `pid`'s sessions were set.
fn set_once(pid: u32, mute: bool) -> Result<u32> {
    // SAFETY: COM on this (blocking-pool) thread's own multithreaded apartment, as network.rs;
    // every interface comes from COM and is released on drop.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let devices = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)?
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let mut muted = 0;
        for d in 0..devices.GetCount()? {
            let sessions = devices.Item(d)?.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)?.GetSessionEnumerator()?;
            for i in 0..sessions.GetCount()? {
                let session = sessions.GetSession(i)?;
                if session.cast::<IAudioSessionControl2>()?.GetProcessId().is_ok_and(|p| p == pid) {
                    session.cast::<ISimpleAudioVolume>()?.SetMute(mute, std::ptr::null())?;
                    muted += 1;
                }
            }
        }
        Ok(muted)
    }
}
