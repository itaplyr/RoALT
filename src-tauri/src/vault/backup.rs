//! Password-protected `.roalt` backup files (portable: no DPAPI).
//!
//! Layout: `b"ROALTBK1" | m_kib u32le | t u32le | p u32le | salt[16] | seal(key, header, json)`
//! where `header` is every byte before the sealed part and `key = Argon2id(password, salt)`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::{Kdf, master};
use crate::error::{AppError, Result};

pub const MAGIC: &[u8; 8] = b"ROALTBK1";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPayload {
    pub version: u32,
    pub exported_at: i64,
    pub accounts: Vec<BackupAccount>,
    pub groups: Vec<BackupGroup>,
    #[serde(default)]
    pub favourites: Vec<BackupFavourite>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFavourite {
    pub name: String,
    pub target: crate::model::LaunchTarget,
    pub user_ids: Vec<i64>,
    pub in_tray: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupAccount {
    pub user_id: i64,
    pub username: String,
    pub display_name: String,
    pub alias: String,
    pub notes: String,
    pub color: Option<String>,
    pub favourite: bool,
    pub sort_order: i64,
    /// Group names.
    pub groups: Vec<String>,
    pub cookie: Zeroizing<String>,
    // Absent in backups from before 0.7; `allow_disconnect` always there after that.
    #[serde(default)]
    pub allow_disconnect: Option<bool>,
    #[serde(default)]
    pub launch_args: Option<String>,
    #[serde(default)]
    pub discord_muted: bool,
    #[serde(default)]
    pub fps_cap: Option<u32>,
    #[serde(default)]
    pub full_quality: bool,
    #[serde(default)]
    pub main: bool,
    // Absent in backups from before custom FFlags: the account's own flags stay then.
    #[serde(default)]
    pub custom_flags: Option<std::collections::BTreeMap<String, String>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupGroup {
    pub name: String,
    pub color: Option<String>,
    pub sort_order: i64,
}

const HEADER_LEN: usize = MAGIC.len() + 3 * 4 + 16;
/// Nonce and tag around the sealed JSON.
const SEAL_OVERHEAD: usize = 12 + 16;
/// Caps on the file's own KDF cost, so a crafted file cannot eat the machine.
const MAX_M_KIB: u32 = 1024 * 1024;
const MAX_T: u32 = 20;
const MAX_P: u32 = 16;

/// Writes atomically (temp file in the same folder, then rename).
pub fn write(path: &Path, password: &str, payload: &BackupPayload) -> Result<()> {
    let kdf = master::new_kdf();
    let mut out = MAGIC.to_vec();
    for v in [kdf.m_kib, kdf.t, kdf.p] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&kdf.salt);
    let json = Zeroizing::new(
        serde_json::to_vec(payload).map_err(|e| {
            log::warn!("could not encode the backup ({:?})", e.classify());
            AppError::Internal(tr!("Could not encode the backup."))
        })?,
    );
    let key = master::derive_key(password, &kdf)?;
    let sealed = master::seal(&key, &out, &json)?;
    out.extend_from_slice(&sealed);
    write_atomic(path, &out)
}

/// `WrongPassword` when the password (or file) does not authenticate,
/// `Invalid("This isn't a RoALT backup.")` on a bad magic / truncated file.
pub fn read(path: &Path, password: &str) -> Result<BackupPayload> {
    let not_backup = || AppError::Invalid(tr!("This isn't a RoALT backup."));
    // About 2 KB per account: far above anyone's vault, and it isn't all read into memory first.
    if fs::metadata(path)?.len() > 64 << 20 {
        return Err(not_backup());
    }
    let bytes = fs::read(path)?;
    if bytes.len() < HEADER_LEN + SEAL_OVERHEAD || !bytes.starts_with(MAGIC) {
        log::debug!("not a backup: bad magic or truncated ({} bytes)", bytes.len());
        return Err(not_backup());
    }
    let (header, sealed) = bytes.split_at(HEADER_LEN);
    let word = |i: usize| u32::from_le_bytes([header[i], header[i + 1], header[i + 2], header[i + 3]]);
    let kdf = Kdf { m_kib: word(8), t: word(12), p: word(16), salt: header[20..].to_vec() };
    if kdf.m_kib > MAX_M_KIB || kdf.t > MAX_T || kdf.p > MAX_P {
        log::debug!("not a backup: absurd KDF cost (m {} KiB, t {}, p {})", kdf.m_kib, kdf.t, kdf.p);
        return Err(not_backup());
    }
    let key = master::derive_key(password, &kdf).map_err(|e| {
        log::debug!("not a backup: key derivation failed: {e}");
        not_backup()
    })?;
    let json = master::open(&key, header, sealed).map_err(|_| AppError::WrongPassword)?;
    // The serde error could quote a cookie: only where it broke is logged.
    serde_json::from_slice(&json).map_err(|e| {
        log::warn!("backup payload unreadable ({:?} at {}:{})", e.classify(), e.line(), e.column());
        not_backup()
    })
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let res = options
        .open(&tmp)
        .and_then(|mut f| {
            f.write_all(bytes)?;
            f.sync_all()
        })
        .and_then(|()| fs::rename(&tmp, path));
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    Ok(res?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "backup pass";

    fn tmp_path() -> PathBuf {
        std::env::temp_dir().join(format!("roalt-test-{}.roalt", uuid::Uuid::new_v4()))
    }

    fn sample() -> BackupPayload {
        BackupPayload {
            favourites: Vec::new(),
            version: 1,
            exported_at: 1_700_000_000,
            accounts: vec![BackupAccount {
                user_id: 42,
                username: "builder".into(),
                display_name: "Builder".into(),
                alias: "main".into(),
                notes: String::new(),
                color: Some("#ff0000".into()),
                favourite: true,
                sort_order: 0,
                groups: vec!["Mains".into()],
                cookie: Zeroizing::new("fake-cookie".into()),
                allow_disconnect: Some(true),
                launch_args: Some("--x".into()),
                discord_muted: true,
                fps_cap: Some(30),
                full_quality: true,
                main: true,
                custom_flags: Some([("FFlagDebugSkyGray".into(), "True".into())].into()),
            }],
            groups: vec![BackupGroup { name: "Mains".into(), color: None, sort_order: 0 }],
        }
    }

    #[test]
    fn older_backups_have_no_custom_flags() {
        let mut v = serde_json::to_value(&sample().accounts[0]).unwrap();
        v.as_object_mut().unwrap().remove("customFlags");
        let a: BackupAccount = serde_json::from_value(v).unwrap();
        assert!(a.custom_flags.is_none(), "None keeps the account's own flags on import");
    }

    #[test]
    fn round_trip_and_wrong_password() {
        let path = tmp_path();
        write(&path, PASSWORD, &sample()).unwrap();
        let tmp_left = PathBuf::from(format!("{}.tmp", path.display())).exists();
        let back = read(&path, PASSWORD);
        let wrong = read(&path, "other pass");
        let _ = fs::remove_file(&path);

        assert!(!tmp_left);
        let back = back.unwrap();
        assert_eq!((back.version, back.exported_at), (1, 1_700_000_000));
        assert_eq!(back.accounts[0].cookie.as_str(), "fake-cookie");
        assert_eq!(back.accounts[0].groups, ["Mains"]);
        assert_eq!(back.accounts[0].custom_flags.as_ref().unwrap()["FFlagDebugSkyGray"], "True");
        assert_eq!(back.groups[0].name, "Mains");
        assert!(matches!(wrong, Err(AppError::WrongPassword)));
    }

    #[test]
    fn a_backup_with_the_removed_macros_still_reads() {
        let old: BackupPayload = serde_json::from_str(r#"{"version":1,"exportedAt":5,"accounts":[],"groups":[],"favourites":[],"macros":[{"name":"x","body":"Send 'w'"}]}"#).unwrap();
        assert_eq!(old.exported_at, 5);
    }

    #[test]
    fn rejects_files_that_are_not_backups() {
        let path = tmp_path();
        write(&path, PASSWORD, &sample()).unwrap();
        let good = fs::read(&path).unwrap();
        let check = |bytes: &[u8]| {
            fs::write(&path, bytes).unwrap();
            read(&path, PASSWORD)
        };
        let mut bad_magic = good.clone();
        bad_magic[0] = b'X';
        let mut absurd_cost = good.clone();
        absurd_cost[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        let rejected = [check(&bad_magic), check(&good[..30]), check(&absurd_cost), check(b"")];
        let cut_body = check(&good[..good.len() - 1]);
        let _ = fs::remove_file(&path);

        for r in rejected {
            assert!(matches!(r, Err(AppError::Invalid(m)) if m == "This isn't a RoALT backup."));
        }
        assert!(matches!(cut_body, Err(AppError::WrongPassword)));
    }
}
