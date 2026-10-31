//! Linux stand-in for Windows DPAPI.
//!
//! There is no per-user OS secret store with a synchronous API here, so the key is derived from
//! stable machine and user identifiers. It is not as strong as DPAPI: a copy of the database and
//! `/etc/machine-id` made on the same machine can be opened on another machine. The vault's
//! secrets still have their own AES-GCM layer, and master-password mode stays portable and
//! protects against offline brute force with Argon2id.

use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::error::Result;
use crate::vault::master;

pub const ENTROPY: &[u8] = b"RoALT/vault/v1";

pub fn protect(data: &[u8]) -> Result<Vec<u8>> {
    protect_with(data, ENTROPY)
}

pub fn protect_with(data: &[u8], entropy: &[u8]) -> Result<Vec<u8>> {
    master::seal(&device_key(entropy), entropy, data)
}

pub fn unprotect(blob_in: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    unprotect_with(blob_in, ENTROPY)
}

pub fn unprotect_with(blob_in: &[u8], entropy: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    master::open(&device_key(entropy), entropy, blob_in)
}

/// A deterministic 256-bit key for this machine, user and purpose (entropy).
fn device_key(entropy: &[u8]) -> Zeroizing<[u8; 32]> {
    let machine = std::fs::read_to_string("/etc/machine-id")
        .or_else(|_| std::fs::read_to_string("/var/lib/dbus/machine-id"))
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| {
            log::warn!("no machine-id; the vault key falls back to the hostname and user");
            format!("{}-{}", std::env::var("HOSTNAME").unwrap_or_default(), std::env::var("USER").unwrap_or_default())
        });
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let mut hasher = Sha256::new();
    hasher.update(b"RoALT/linux-device-key/v1\0");
    hasher.update(machine.as_bytes());
    hasher.update(b"\0");
    hasher.update(uid.to_le_bytes());
    hasher.update(b"\0");
    hasher.update(entropy);
    let digest = hasher.finalize();
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&digest);
    let mut digest = digest;
    digest.zeroize();
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    #[test]
    fn round_trip_and_aad_binding() {
        let wrapped = protect(b"data key").unwrap();
        assert_ne!(wrapped.as_slice(), b"data key");
        assert_eq!(unprotect(&wrapped).unwrap().as_slice(), b"data key");
        assert!(matches!(unprotect_with(&wrapped, b"other"), Err(AppError::Crypto)));
        assert!(matches!(unprotect(b"not a blob"), Err(AppError::Crypto)));
    }
}
