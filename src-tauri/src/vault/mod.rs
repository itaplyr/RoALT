//! Envelope encryption for everything secret.
//!
//! One random 256-bit data key encrypts every secret (AES-256-GCM, random 96-bit nonce per
//! value, AAD = row identity). The data key itself is stored wrapped:
//! - `Dpapi`: `DPAPI(data key)` for the current Windows user; opens without a prompt.
//! - `Master`: `DPAPI(seal(Argon2id(password, salt), data key))`; locked until `unlock`.
//!
//! A master vault may also have a recovery record, the same wrap under a one-use code:
//! `DPAPI(seal(Argon2id(code, salt), data key))`. The code itself is never stored.
//!
//! Switching mode only re-wraps the data key.

pub mod backup;
#[cfg(windows)]
mod dpapi;
#[cfg(not(windows))]
#[path = "dpapi_linux.rs"]
mod dpapi;
#[cfg(windows)]
mod hello;
#[cfg(not(windows))]
#[path = "hello_linux.rs"]
mod hello;
pub mod master;

use std::fmt;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use zeroize::Zeroizing;

use crate::error::{AppError, Result};
use crate::model::VaultMode;

pub use dpapi::{protect_with, unprotect_with};
pub use hello::{hello_available, windows_hello};

/// A secret string (cookie, password). Zeroed on drop, never printed.
#[derive(Clone)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Self(Zeroizing::new(s.into()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// AAD for a stored cookie, so a blob cannot be moved to another account's row.
pub fn cookie_aad(user_id: i64) -> Vec<u8> {
    format!("roalt.cookie:{user_id}").into_bytes()
}

/// Argon2id parameters stored next to a master-password vault.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kdf {
    pub salt: Vec<u8>,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

/// The `vault` table row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultRecord {
    pub mode: VaultMode,
    pub wrapped_key: Vec<u8>,
    /// `Some` in master mode only.
    pub kdf: Option<Kdf>,
}

/// The `vault_recovery` table row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryRecord {
    pub wrapped_key: Vec<u8>,
    pub kdf: Kdf,
}

struct Inner {
    mode: VaultMode,
    key: Option<Zeroizing<[u8; 32]>>,
    last_activity: Instant,
}

pub struct Vault {
    inner: Mutex<Inner>,
}

/// Shortest master password accepted.
pub const MIN_MASTER_PASSWORD: usize = 8;

/// AAD for the sealed data key in master mode.
const KEY_AAD: &[u8] = b"roalt.vault.key";
/// AAD for the sealed data key in a recovery record.
const RECOVERY_AAD: &[u8] = b"roalt.vault.recovery";

type Key = Zeroizing<[u8; 32]>;

impl Vault {
    /// A new vault: random data key, DPAPI-wrapped, returned unlocked.
    pub fn create() -> Result<(Vault, VaultRecord)> {
        let key = Zeroizing::new(master::random_bytes::<32>());
        let rec = wrap_dpapi(&key)?;
        Ok((Vault::with(VaultMode::Dpapi, Some(key)), rec))
    }

    /// DPAPI mode unwraps at once (unlocked). Master mode stays locked until `unlock`.
    pub fn open(rec: &VaultRecord) -> Result<Vault> {
        let key = match rec.mode {
            VaultMode::Dpapi => Some(unwrap_dpapi(rec)?),
            VaultMode::Master => None,
        };
        Ok(Vault::with(rec.mode, key))
    }

    fn with(mode: VaultMode, key: Option<Key>) -> Vault {
        Vault { inner: Mutex::new(Inner { mode, key, last_activity: Instant::now() }) }
    }

    fn guard(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| {
            log::warn!("vault lock was poisoned by an earlier panic; recovering");
            // Cleared so this is logged once, not on every call.
            self.inner.clear_poison();
            e.into_inner()
        })
    }

    /// A copy of the data key, or `Locked`.
    fn key(&self) -> Result<Key> {
        self.guard().key.clone().ok_or(AppError::Locked)
    }

    pub fn mode(&self) -> VaultMode {
        self.guard().mode
    }

    pub fn is_locked(&self) -> bool {
        self.guard().key.is_none()
    }

    /// Master mode: derive, unwrap and keep the data key. `WrongPassword` on failure.
    pub fn unlock(&self, rec: &VaultRecord, password: &str) -> Result<()> {
        let key = match rec.mode {
            VaultMode::Dpapi => unwrap_dpapi(rec)?,
            VaultMode::Master => unwrap_master(rec, password)?,
        };
        let mut g = self.guard();
        g.key = Some(key);
        g.last_activity = Instant::now();
        Ok(())
    }

    /// Drops (zeroes) the data key. No-op in DPAPI mode, which cannot be locked.
    pub fn lock(&self) {
        let mut g = self.guard();
        if g.mode == VaultMode::Master {
            g.key = None;
        }
    }

    /// Re-authentication in master mode (reveal / copy cookie). `WrongPassword` on mismatch.
    pub fn check_password(&self, rec: &VaultRecord, password: &str) -> Result<()> {
        unwrap_master(rec, password).map(drop)
    }

    /// Re-wraps the (unlocked) data key for `mode` and returns the new record.
    /// Leaving master mode needs `current_password`; entering or changing it needs
    /// `new_password` (at least `MIN_MASTER_PASSWORD` chars).
    pub fn rewrap(
        &self,
        rec: &VaultRecord,
        mode: VaultMode,
        current_password: Option<&str>,
        new_password: Option<&str>,
    ) -> Result<VaultRecord> {
        let key = self.key()?;
        let new_password = match mode {
            VaultMode::Master => Some(long_enough(new_password)?),
            VaultMode::Dpapi => None,
        };
        if rec.mode == VaultMode::Master {
            self.check_password(rec, current_password.ok_or(AppError::WrongPassword)?)?;
        }
        match new_password {
            Some(pw) => wrap_master(&key, pw),
            None => wrap_dpapi(&key),
        }
    }

    /// A new recovery code for the (unlocked) data key, and the record it opens. Saving the record
    /// replaces the old one, which revokes the old code.
    pub fn seal_recovery(&self) -> Result<(Secret, RecoveryRecord)> {
        wrap_recovery(&*self.key()?)
    }

    /// Switches the in-memory mode; call only after the record from `rewrap` is saved.
    pub fn set_mode(&self, mode: VaultMode) {
        self.guard().mode = mode;
    }

    /// `Locked` when locked.
    pub fn encrypt(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
        let key = self.key()?;
        master::seal(&key, aad, plaintext)
    }

    /// `Locked` when locked, `Crypto` when the blob or AAD does not match.
    pub fn decrypt(&self, aad: &[u8], blob: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let key = self.key()?;
        master::open(&key, aad, blob)
    }

    pub fn decrypt_secret(&self, aad: &[u8], blob: &[u8]) -> Result<Secret> {
        let bytes = self.decrypt(aad, blob)?;
        let s = std::str::from_utf8(&bytes).map_err(|e| {
            log::warn!("decrypted secret is not UTF-8 (valid up to byte {})", e.valid_up_to());
            AppError::Crypto
        })?;
        Ok(Secret::new(s))
    }

    /// Marks user activity (auto-lock).
    pub fn touch(&self) {
        self.guard().last_activity = Instant::now();
    }

    pub fn idle(&self) -> Duration {
        self.guard().last_activity.elapsed()
    }
}

/// Opens `rec` with `code` and re-wraps the data key: under `new_password`, and under a fresh code
/// since this one is now used. Returns the new vault record, code and recovery record; nothing
/// changes until those are saved. `WrongPassword` when `code` does not open it.
pub fn recover(rec: &RecoveryRecord, code: &str, new_password: &str) -> Result<(VaultRecord, Secret, RecoveryRecord)> {
    let new_password = long_enough(Some(new_password))?;
    let code = master::normalize_code(code).ok_or(AppError::WrongPassword)?;
    let sealed = dpapi::unprotect(&rec.wrapped_key)?;
    let kek = master::derive_key(code.expose(), &rec.kdf)?;
    let key = to_key(&master::open(&kek, RECOVERY_AAD, &sealed).map_err(|_| AppError::WrongPassword)?)?;
    let vault = wrap_master(&key, new_password)?;
    let (new_code, recovery) = wrap_recovery(&key)?;
    Ok((vault, new_code, recovery))
}

fn long_enough(password: Option<&str>) -> Result<&str> {
    password.filter(|p| p.chars().count() >= MIN_MASTER_PASSWORD).ok_or_else(|| {
        AppError::Invalid(tr!("Use at least {min} characters for the master password.", min = MIN_MASTER_PASSWORD))
    })
}

fn wrap_recovery(key: &[u8; 32]) -> Result<(Secret, RecoveryRecord)> {
    let code = master::recovery_code();
    let kdf = master::new_recovery_kdf();
    let kek = master::derive_key(code.expose(), &kdf)?;
    let sealed = master::seal(&kek, RECOVERY_AAD, key)?;
    Ok((code, RecoveryRecord { wrapped_key: dpapi::protect(&sealed)?, kdf }))
}

fn wrap_dpapi(key: &[u8; 32]) -> Result<VaultRecord> {
    Ok(VaultRecord { mode: VaultMode::Dpapi, wrapped_key: dpapi::protect(key)?, kdf: None })
}

fn wrap_master(key: &[u8; 32], password: &str) -> Result<VaultRecord> {
    let kdf = master::new_kdf();
    let kek = master::derive_key(password, &kdf)?;
    let sealed = master::seal(&kek, KEY_AAD, key)?;
    Ok(VaultRecord { mode: VaultMode::Master, wrapped_key: dpapi::protect(&sealed)?, kdf: Some(kdf) })
}

fn unwrap_dpapi(rec: &VaultRecord) -> Result<Key> {
    to_key(&dpapi::unprotect(&rec.wrapped_key)?)
}

/// `WrongPassword` when `password` does not open the sealed data key.
fn unwrap_master(rec: &VaultRecord, password: &str) -> Result<Key> {
    let kdf = rec.kdf.as_ref().ok_or_else(|| {
        log::error!("vault record is in master mode but has no KDF parameters");
        AppError::Internal(tr!("No master password is set."))
    })?;
    let sealed = dpapi::unprotect(&rec.wrapped_key)?;
    let kek = master::derive_key(password, kdf)?;
    to_key(&master::open(&kek, KEY_AAD, &sealed).map_err(|_| AppError::WrongPassword)?)
}

fn to_key(bytes: &[u8]) -> Result<Key> {
    if bytes.len() != 32 {
        log::error!("vault data key has the wrong length ({} bytes)", bytes.len());
        return Err(AppError::Crypto);
    }
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(bytes);
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AAD: &[u8] = b"roalt.cookie:1";

    #[test]
    fn create_encrypt_decrypt_and_reopen() {
        let (v, rec) = Vault::create().unwrap();
        assert_eq!((v.mode(), v.is_locked(), rec.kdf.is_none()), (VaultMode::Dpapi, false, true));
        let blob = v.encrypt(AAD, b"cookie").unwrap();
        assert_eq!(v.decrypt(AAD, &blob).unwrap().as_slice(), b"cookie");
        assert!(matches!(v.decrypt(b"roalt.cookie:2", &blob), Err(AppError::Crypto)));
        v.lock();
        let reopened = Vault::open(&rec).unwrap();
        assert_eq!(reopened.decrypt_secret(AAD, &blob).unwrap().expose(), "cookie");
    }

    #[test]
    fn master_mode_round_trip() {
        let (v, rec) = Vault::create().unwrap();
        let blob = v.encrypt(AAD, b"cookie").unwrap();

        assert!(matches!(v.rewrap(&rec, VaultMode::Master, None, Some("short")), Err(AppError::Invalid(_))));
        let rec = v.rewrap(&rec, VaultMode::Master, None, Some("correct horse")).unwrap();
        v.set_mode(rec.mode);
        assert_eq!((v.mode(), rec.mode, rec.kdf.is_some()), (VaultMode::Master, VaultMode::Master, true));

        v.lock();
        assert!(v.is_locked());
        assert!(matches!(v.decrypt(AAD, &blob), Err(AppError::Locked)));
        assert!(matches!(v.rewrap(&rec, VaultMode::Dpapi, Some("correct horse"), None), Err(AppError::Locked)));
        assert!(matches!(v.unlock(&rec, "wrong password"), Err(AppError::WrongPassword)));

        let reopened = Vault::open(&rec).unwrap();
        assert!(reopened.is_locked());
        reopened.unlock(&rec, "correct horse").unwrap();
        assert_eq!(reopened.decrypt(AAD, &blob).unwrap().as_slice(), b"cookie");
        assert!(matches!(reopened.check_password(&rec, "nope nope"), Err(AppError::WrongPassword)));

        let changed = reopened.rewrap(&rec, VaultMode::Master, Some("correct horse"), Some("battery staple")).unwrap();
        assert!(matches!(reopened.unlock(&changed, "correct horse"), Err(AppError::WrongPassword)));
        let back = reopened.rewrap(&changed, VaultMode::Dpapi, Some("correct horse"), None);
        assert!(matches!(back, Err(AppError::WrongPassword)));

        let back = reopened.rewrap(&changed, VaultMode::Dpapi, Some("battery staple"), None).unwrap();
        reopened.set_mode(back.mode);
        assert_eq!((reopened.mode(), back.mode, back.kdf.is_none()), (VaultMode::Dpapi, VaultMode::Dpapi, true));
        assert_eq!(Vault::open(&back).unwrap().decrypt(AAD, &blob).unwrap().as_slice(), b"cookie");
    }

    /// A master vault holding one secret, locked again, with the record its code opens.
    fn locked_with_code() -> (VaultRecord, Vec<u8>, Secret, RecoveryRecord) {
        let (v, rec) = Vault::create().unwrap();
        let blob = v.encrypt(AAD, b"cookie").unwrap();
        let rec = v.rewrap(&rec, VaultMode::Master, None, Some("forgotten one")).unwrap();
        let (code, recovery) = v.seal_recovery().unwrap();
        (rec, blob, code, recovery)
    }

    fn opens(rec: &VaultRecord, password: &str, blob: &[u8]) -> bool {
        let v = Vault::open(rec).unwrap();
        v.unlock(rec, password).is_ok() && v.decrypt(AAD, blob).unwrap().as_slice() == b"cookie"
    }

    #[test]
    fn recover_round_trip() {
        let (rec, blob, code, recovery) = locked_with_code();

        let wrong = if code.expose() == "222-222-222" { "333-333-333" } else { "222-222-222" };
        assert!(matches!(recover(&recovery, wrong, "brand new pass"), Err(AppError::WrongPassword)));
        assert!(matches!(recover(&recovery, "not a code", "brand new pass"), Err(AppError::WrongPassword)));
        assert!(matches!(recover(&recovery, code.expose(), "short"), Err(AppError::Invalid(_))));

        let typed = code.expose().replace('-', " ").to_lowercase();
        let (new_rec, new_code, new_recovery) = recover(&recovery, &typed, "brand new pass").unwrap();
        assert_eq!(new_rec.mode, VaultMode::Master);
        assert!(opens(&new_rec, "brand new pass", &blob), "new password and the data key work");
        assert!(!opens(&new_rec, "forgotten one", &blob), "old password is void");
        assert!(opens(&rec, "forgotten one", &blob), "nothing changes until the records are saved");

        assert_ne!(new_code.expose(), code.expose());
        assert!(matches!(recover(&new_recovery, code.expose(), "another pass"), Err(AppError::WrongPassword)), "old code is void");
        let (again, _, _) = recover(&new_recovery, new_code.expose(), "another pass").unwrap();
        assert!(opens(&again, "another pass", &blob), "the fresh code works");
    }

    #[test]
    fn password_change_keeps_the_code_working() {
        let (rec, blob, code, recovery) = locked_with_code();
        let v = Vault::open(&rec).unwrap();
        v.unlock(&rec, "forgotten one").unwrap();
        let changed = v.rewrap(&rec, VaultMode::Master, Some("forgotten one"), Some("changed password")).unwrap();
        assert!(opens(&changed, "changed password", &blob));
        let (recovered, _, _) = recover(&recovery, code.expose(), "brand new pass").unwrap();
        assert!(opens(&recovered, "brand new pass", &blob));
    }

    #[test]
    fn replacing_revokes_the_old_code() {
        let (rec, blob, old_code, _) = locked_with_code();
        let v = Vault::open(&rec).unwrap();
        v.unlock(&rec, "forgotten one").unwrap();
        let (new_code, replaced) = v.seal_recovery().unwrap();
        assert!(matches!(recover(&replaced, old_code.expose(), "brand new pass"), Err(AppError::WrongPassword)));
        let (recovered, _, _) = recover(&replaced, new_code.expose(), "brand new pass").unwrap();
        assert!(opens(&recovered, "brand new pass", &blob));
    }

    #[test]
    fn a_locked_vault_cannot_seal_a_code() {
        let (rec, ..) = locked_with_code();
        assert!(matches!(Vault::open(&rec).unwrap().seal_recovery(), Err(AppError::Locked)));
    }

    #[test]
    fn a_password_wrap_is_not_a_recovery_wrap() {
        let (_, _, code, recovery) = locked_with_code();
        let kek = master::derive_key(code.expose(), &recovery.kdf).unwrap();
        let sealed = dpapi::unprotect(&recovery.wrapped_key).unwrap();
        assert!(master::open(&kek, KEY_AAD, &sealed).is_err(), "bound to its own AAD");
    }
}
