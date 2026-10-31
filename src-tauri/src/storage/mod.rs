//! SQLite storage. One connection behind a mutex; every query is short. A database file is
//! encrypted whole (`cipher`).
// ponytail: queries run on the calling (tokio) thread; move to spawn_blocking if any grows slow.

mod cipher;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Row, params};
use rusqlite_migration::{M, Migrations};

use crate::commands::now_ms;
use crate::error::{AppError, Result};
use crate::model::{AccountPatch, AccountStatus, AccountSummary, Crash, CrashKind, Group, GroupInput, ServerVisit, Settings, VaultMode};
use crate::vault::{Kdf, RecoveryRecord, VaultRecord};

/// What `upsert_account` writes. `cookie` is the encrypted blob.
pub struct NewAccount<'a> {
    pub user_id: i64,
    pub username: &'a str,
    pub display_name: &'a str,
    pub cookie: &'a [u8],
    pub status: AccountStatus,
    pub checked_at: Option<i64>,
}

/// User-editable details, set wholesale by a backup import.
pub struct AccountMeta {
    pub alias: String,
    pub notes: String,
    pub color: Option<String>,
    pub favourite: bool,
}

pub struct Db {
    conn: Mutex<Connection>,
    /// The database file (its key is `cipher::read_key(file)`); None in memory.
    file: Option<PathBuf>,
}

/// The key file beside a database: `roalt.db` → `roalt.db.key`.
pub fn key_path(db: &Path) -> PathBuf {
    cipher::beside(db, ".key")
}

/// A `favourites` row; `target` is the encrypted `LaunchTarget` JSON (link codes are bearer secrets).
pub struct FavouriteRow {
    pub id: i64,
    pub name: String,
    pub target: Vec<u8>,
    pub account_ids: Vec<i64>,
    pub in_tray: bool,
    pub sort_order: i64,
    pub last_used_at: Option<i64>,
}

/// Set once `Db::public_guard_off_once` has run.
const PUBLIC_GUARD_OFF_ONCE: &str = "public_guard_default_v2";
/// Set once `Db::background_cover_once` has run.
const BACKGROUND_COVER_ONCE: &str = "background_cover_v2";
const MAX_ALIAS: usize = 64;
const MAX_NOTES: usize = 2000;
const MAX_GROUP_NAME: usize = 40;
const MAX_CRASHES: i64 = 200;
const MAX_SERVERS: i64 = 50;

/// What `add_crash` writes: one client that ended badly. `log_path` never reaches the UI.
pub struct NewCrash<'a> {
    pub instance_id: &'a str,
    pub account_id: i64,
    pub kind: CrashKind,
    pub at: i64,
    pub uptime_ms: i64,
    pub reason: Option<&'a str>,
    pub place_id: Option<i64>,
    pub log_path: Option<&'a str>,
}

const ACCOUNT_SELECT: &str = "SELECT a.id, a.user_id, a.username, a.display_name, a.alias, a.notes, a.color, \
     a.favourite, a.sort_order, \
     (SELECT group_concat(ag.group_id ORDER BY g.sort_order, g.id) \
        FROM account_groups ag JOIN groups g ON g.id = ag.group_id WHERE ag.account_id = a.id), \
     a.status, a.status_checked_at, a.created_at, a.allow_disconnect, a.launch_args, a.discord_muted, a.fps_cap, a.full_quality \
     FROM accounts a";

const GROUP_SELECT: &str = "SELECT id, name, color, sort_order FROM groups";

impl Db {
    /// Opens (creating if needed) and migrates. Encrypted (`cipher`: a new database gets a new
    /// key, a plain one from before is encrypted first), WAL, foreign keys on.
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        cipher::resume(path)?;
        let key = if key_path(path).exists() {
            cipher::read_key(path)?
        } else if path.exists() {
            cipher::migrate(path)?
        } else {
            cipher::new_key(&key_path(path))?
        };
        let conn = Connection::open(path)?;
        cipher::unlock(&conn, &key)?;
        drop(key);
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        Db::init(conn, Some(path.to_owned()))
    }

    pub fn open_in_memory() -> Result<Db> {
        Db::init(Connection::open_in_memory()?, None)
    }

    fn init(mut conn: Connection, file: Option<PathBuf>) -> Result<Db> {
        conn.busy_timeout(Duration::from_secs(5))?;
        // secure_delete: freed pages (removed cookies, old key wraps) are zeroed, not left behind.
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA secure_delete = ON;")?;
        migrations()
            .to_latest(&mut conn)
            .map_err(|e| {
                log::error!("database migration failed: {e}");
                AppError::Storage(e.to_string())
            })?;
        let db = Db { conn: Mutex::new(conn), file };
        db.public_guard_off_once()?;
        db.background_cover_once()?;
        Ok(db)
    }

    /// The public-network guard is off by default now. A stored "guard on" can't be told apart
    /// from the old default nobody chose, so it goes off once for everyone; choices made after
    /// that stay. Corrupt settings are left for `settings` to report.
    fn public_guard_off_once(&self) -> Result<()> {
        if self.setting_value(PUBLIC_GUARD_OFF_ONCE)?.is_some() {
            return Ok(());
        }
        if let Some(mut s) = self.setting_value("settings")?.and_then(|j| serde_json::from_str::<Settings>(&j).ok())
            && !s.allow_public_networks
        {
            s.allow_public_networks = true;
            self.save_settings(&s)?;
        }
        self.set_setting_value(PUBLIC_GUARD_OFF_ONCE, Some("1"))
    }

    /// The theme used to cover a background image by 70 to 90 %, which hid most pictures. A stored
    /// value over today's default goes down to it once; choices made after that stay.
    fn background_cover_once(&self) -> Result<()> {
        if self.setting_value(BACKGROUND_COVER_ONCE)?.is_some() {
            return Ok(());
        }
        let cover = Settings::default().background_dim;
        if let Some(mut s) = self.setting_value("settings")?.and_then(|j| serde_json::from_str::<Settings>(&j).ok())
            && s.background_dim > cover
        {
            s.background_dim = cover;
            self.save_settings(&s)?;
        }
        self.set_setting_value(BACKGROUND_COVER_ONCE, Some("1"))
    }

    /// A consistent copy of the open database, written to `path` (compacted: no freed pages
    /// come along) and encrypted with the same key, which goes beside it (`key_path`); then
    /// `then` with the database still locked, so nothing is written to it after the copy
    /// meanwhile. Fails when `path` exists.
    pub fn copy_to<T>(&self, path: &Path, then: impl FnOnce() -> Result<T>) -> Result<T> {
        if path.exists() {
            return Err(AppError::Invalid(tr!("That folder already has a RoALT database.")));
        }
        let conn = self.conn();
        match &self.file {
            Some(file) => {
                let key = cipher::read_key(file)?;
                // Under its final name only when whole: a kill here must not leave a folder
                // that looks as if it already had a database.
                let partial = path.with_extension("db.partial");
                let _ = std::fs::remove_file(&partial);
                cipher::export(&conn, &partial, &key)?;
                cipher::write_key(&key_path(path), &key)?;
                std::fs::rename(&partial, path)?;
            }
            None => {
                let path = path.to_str().ok_or_else(|| AppError::Invalid(tr!("That folder's name can't be used.")))?;
                conn.execute("VACUUM INTO ?1", [path])?;
            }
        }
        let out = then();
        drop(conn);
        out
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| {
            log::warn!("database lock was poisoned by an earlier panic; recovering");
            // Cleared so this is logged once, not on every query.
            self.conn.clear_poison();
            e.into_inner()
        })
    }

    pub fn vault_record(&self) -> Result<Option<VaultRecord>> {
        let sql = "SELECT mode, wrapped_key, kdf_salt, kdf_m_kib, kdf_t, kdf_p FROM vault WHERE id = 1";
        let rec = self
            .conn()
            .query_row(sql, [], |r| {
                let mode = match r.get::<_, String>(0)?.as_str() {
                    "master" => VaultMode::Master,
                    "dpapi" => VaultMode::Dpapi,
                    other => {
                        log::warn!("unknown vault mode {other:?}; treating it as dpapi");
                        VaultMode::Dpapi
                    }
                };
                let kdf = match (r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?) {
                    (Some(salt), Some(m_kib), Some(t), Some(p)) => Some(Kdf { salt, m_kib, t, p }),
                    _ => None,
                };
                Ok(VaultRecord { mode, wrapped_key: r.get(1)?, kdf })
            })
            .optional()?;
        Ok(rec)
    }

    pub fn save_vault_record(&self, rec: &VaultRecord) -> Result<()> {
        self.save_vault(rec, None)
    }

    /// Both or neither: a recovery is only done, and its old code void, with the new password.
    pub fn save_vault_and_recovery(&self, rec: &VaultRecord, recovery: &RecoveryRecord) -> Result<()> {
        self.save_vault(rec, Some(recovery))
    }

    fn save_vault(&self, rec: &VaultRecord, recovery: Option<&RecoveryRecord>) -> Result<()> {
        let (mode, kdf) = match rec.mode {
            VaultMode::Dpapi => ("dpapi", None),
            VaultMode::Master => ("master", rec.kdf.as_ref()),
        };
        let mut c = self.conn();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO vault (id, mode, wrapped_key, kdf_salt, kdf_m_kib, kdf_t, kdf_p) \
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
            params![mode, rec.wrapped_key, kdf.map(|k| &k.salt), kdf.map(|k| k.m_kib), kdf.map(|k| k.t), kdf.map(|k| k.p)],
        )?;
        // A recovery code belongs to the master password: it goes when that does.
        if rec.mode == VaultMode::Dpapi {
            tx.execute("DELETE FROM vault_recovery", [])?;
        }
        if let Some(r) = recovery {
            write_recovery(&tx, r)?;
        }
        tx.commit()?;
        flush_old_wraps(&c)
    }

    pub fn recovery_record(&self) -> Result<Option<RecoveryRecord>> {
        let sql = "SELECT wrapped_key, kdf_salt, kdf_m_kib, kdf_t, kdf_p FROM vault_recovery WHERE id = 1";
        let rec = self
            .conn()
            .query_row(sql, [], |r| {
                Ok(RecoveryRecord { wrapped_key: r.get(0)?, kdf: Kdf { salt: r.get(1)?, m_kib: r.get(2)?, t: r.get(3)?, p: r.get(4)? } })
            })
            .optional()?;
        Ok(rec)
    }

    /// Replaces the record, which revokes the old code.
    pub fn save_recovery_record(&self, recovery: &RecoveryRecord) -> Result<()> {
        let c = self.conn();
        write_recovery(&c, recovery)?;
        flush_old_wraps(&c)
    }

    /// Ordered by `sort_order`, then `id`.
    pub fn accounts(&self) -> Result<Vec<AccountSummary>> {
        let c = self.conn();
        let mut st = c.prepare(&format!("{ACCOUNT_SELECT} ORDER BY a.sort_order, a.id"))?;
        Ok(st.query_map([], account_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// `NotFound` if missing.
    pub fn account(&self, id: i64) -> Result<AccountSummary> {
        Ok(self.conn().query_row(&format!("{ACCOUNT_SELECT} WHERE a.id = ?1"), [id], account_row)?)
    }

    /// The accounts marked Full quality (read every poll: only their ids).
    pub fn full_quality_accounts(&self) -> Result<Vec<i64>> {
        let c = self.conn();
        let mut st = c.prepare("SELECT id FROM accounts WHERE full_quality = 1")?;
        Ok(st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn account_id_for_user(&self, user_id: i64) -> Result<Option<i64>> {
        let sql = "SELECT id FROM accounts WHERE user_id = ?1";
        Ok(self.conn().query_row(sql, [user_id], |r| r.get(0)).optional()?)
    }

    /// Inserts, or updates username / display name / cookie / status of the row with this
    /// `user_id` (keeping alias, notes, colour, favourite, groups, order). New rows go last.
    /// Returns the account id.
    pub fn upsert_account(&self, a: &NewAccount) -> Result<i64> {
        // One statement, so atomic without an explicit transaction.
        let sql = "INSERT INTO accounts \
             (id, user_id, username, display_name, cookie, status, status_checked_at, sort_order, created_at, updated_at) \
             VALUES (MAX((SELECT COALESCE(MAX(id), 0) FROM accounts), \
                         (SELECT COALESCE(CAST(MAX(value) AS INTEGER), 0) FROM settings WHERE key = 'last_account_id')) + 1, \
                     ?1, ?2, ?3, ?4, ?5, ?6, (SELECT COALESCE(MAX(sort_order) + 1, 0) FROM accounts), ?7, ?7) \
             ON CONFLICT (user_id) DO UPDATE SET username = excluded.username, \
             display_name = excluded.display_name, cookie = excluded.cookie, status = excluded.status, \
             status_checked_at = excluded.status_checked_at, updated_at = excluded.updated_at \
             RETURNING id";
        let p = params![a.user_id, a.username, a.display_name, a.cookie, a.status.as_str(), a.checked_at, now_ms()];
        Ok(self.conn().query_row(sql, p, |r| r.get(0))?)
    }

    pub fn set_meta(&self, id: i64, meta: &AccountMeta) -> Result<()> {
        write_meta(&self.conn(), id, meta)
    }

    /// Applies a UI patch. `color` must be `#rrggbb` or `""` (clear); alias ≤ 64 chars,
    /// notes ≤ 2000 chars; `group_ids` must exist and replace the current set; `launch_args`
    /// blank clears the override, `fps_cap` 0 likewise (the caller validates the rest).
    /// Bad input → `AppError::Invalid`, unknown id → `NotFound`.
    pub fn update_account(&self, id: i64, patch: &AccountPatch) -> Result<()> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        let sql = "SELECT alias, notes, color, favourite FROM accounts WHERE id = ?1";
        let mut m = tx.query_row(sql, [id], |r| {
            Ok(AccountMeta { alias: r.get(0)?, notes: r.get(1)?, color: r.get(2)?, favourite: r.get(3)? })
        })?;
        if let Some(v) = &patch.alias {
            m.alias = v.clone();
        }
        if let Some(v) = &patch.notes {
            m.notes = v.clone();
        }
        if let Some(v) = &patch.color {
            m.color = Some(v.clone());
        }
        if let Some(v) = patch.favourite {
            m.favourite = v;
        }
        write_meta(&tx, id, &m)?;
        if let Some(ids) = &patch.group_ids {
            replace_groups(&tx, id, ids)?;
        }
        if let Some(v) = patch.allow_disconnect {
            tx.execute("UPDATE accounts SET allow_disconnect = ?2 WHERE id = ?1", params![id, v])?;
        }
        if let Some(v) = &patch.launch_args {
            let v = Some(v.trim()).filter(|v| !v.is_empty());
            tx.execute("UPDATE accounts SET launch_args = ?2 WHERE id = ?1", params![id, v])?;
        }
        if let Some(v) = patch.discord_muted {
            tx.execute("UPDATE accounts SET discord_muted = ?2 WHERE id = ?1", params![id, v])?;
        }
        if let Some(v) = patch.fps_cap {
            tx.execute("UPDATE accounts SET fps_cap = ?2 WHERE id = ?1", params![id, Some(v).filter(|&v| v != 0)])?;
        }
        if let Some(v) = patch.full_quality {
            tx.execute("UPDATE accounts SET full_quality = ?2 WHERE id = ?1", params![id, v])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// `(user_id, encrypted cookie)`.
    pub fn account_cookie(&self, id: i64) -> Result<(i64, Vec<u8>)> {
        let sql = "SELECT user_id, cookie FROM accounts WHERE id = ?1";
        Ok(self.conn().query_row(sql, [id], |r| Ok((r.get(0)?, r.get(1)?)))?)
    }

    pub fn set_cookie(&self, id: i64, blob: &[u8]) -> Result<()> {
        let sql = "UPDATE accounts SET cookie = ?2, updated_at = ?3 WHERE id = ?1";
        found(self.conn().execute(sql, params![id, blob, now_ms()])?)
    }

    /// Also refreshes username / display name when given.
    pub fn set_status(
        &self,
        id: i64,
        status: AccountStatus,
        checked_at: i64,
        username: Option<&str>,
        display_name: Option<&str>,
    ) -> Result<()> {
        let sql = "UPDATE accounts SET status = ?2, status_checked_at = ?3, username = COALESCE(?4, username), \
             display_name = COALESCE(?5, display_name), updated_at = ?6 WHERE id = ?1";
        found(self.conn().execute(sql, params![id, status.as_str(), checked_at, username, display_name, now_ms()])?)
    }

    /// `ids` get sort_order 0..n in that order; accounts not listed keep their relative
    /// order after them. One transaction.
    pub fn reorder_accounts(&self, ids: &[i64]) -> Result<()> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        let current: Vec<i64> = tx
            .prepare("SELECT id FROM accounts ORDER BY sort_order, id")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        // ponytail: O(n²) contains checks; fine for hundreds of accounts, use a HashSet past that.
        let mut order: Vec<i64> = Vec::with_capacity(current.len());
        for id in ids.iter().chain(&current) {
            if current.contains(id) && !order.contains(id) {
                order.push(*id);
            }
        }
        for (i, id) in order.iter().enumerate() {
            tx.execute("UPDATE accounts SET sort_order = ?1 WHERE id = ?2", params![i as i64, id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_accounts(&self, ids: &[i64]) -> Result<()> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        for id in ids {
            // Cached Roblox data (Robux, 2-step, presence) goes with the account.
            tx.execute(
                "DELETE FROM cache WHERE key IN \
                 (SELECT 'info:' || user_id FROM accounts WHERE id = ?1 \
                  UNION ALL SELECT 'headshot:' || user_id FROM accounts WHERE id = ?1)",
                [id],
            )?;
            // An id is never used again: a late cookie rotation, a running client or a favourite
            // must not reach whoever is added next.
            tx.execute(
                "INSERT INTO settings (key, value) VALUES ('last_account_id', ?1) ON CONFLICT (key) \
                 DO UPDATE SET value = MAX(CAST(value AS INTEGER), ?1)",
                [id],
            )?;
            tx.execute("UPDATE favourites SET account_ids = (SELECT json_group_array(value) FROM json_each(account_ids) WHERE value <> ?1)", [id])?;
            tx.execute("DELETE FROM accounts WHERE id = ?1", [id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn groups(&self) -> Result<Vec<Group>> {
        let c = self.conn();
        let mut st = c.prepare(&format!("{GROUP_SELECT} ORDER BY sort_order, id"))?;
        Ok(st.query_map([], group_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Name trimmed, 1..=40 chars, unique (case-insensitive) → else `Invalid`.
    /// Colour `#rrggbb` or none.
    pub fn save_group(&self, g: &GroupInput) -> Result<Group> {
        let c = self.conn();
        let id = write_group(&c, g.id, &g.name, g.color.as_deref())?;
        Ok(c.query_row(&format!("{GROUP_SELECT} WHERE id = ?1"), [id], group_row)?)
    }

    pub fn remove_group(&self, id: i64) -> Result<()> {
        self.conn().execute("DELETE FROM groups WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Existing group id for this name (case-insensitive), or a new group.
    pub fn group_by_name_or_create(&self, name: &str, color: Option<&str>) -> Result<i64> {
        let c = self.conn();
        let sql = "SELECT id FROM groups WHERE name = ?1";
        match c.query_row(sql, [name.trim()], |r| r.get(0)).optional()? {
            Some(id) => Ok(id),
            None => write_group(&c, None, name, color),
        }
    }

    /// Replaces the account's group set.
    pub fn set_account_groups(&self, account_id: i64, group_ids: &[i64]) -> Result<()> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        found(tx.execute("UPDATE accounts SET updated_at = ?2 WHERE id = ?1", [account_id, now_ms()])?)?;
        replace_groups(&tx, account_id, group_ids)?;
        tx.commit()?;
        Ok(())
    }

    /// Defaults when nothing is stored yet (key `settings`, JSON).
    pub fn settings(&self) -> Result<Settings> {
        let sql = "SELECT value FROM settings WHERE key = 'settings'";
        let json: Option<String> = self.conn().query_row(sql, [], |r| r.get(0)).optional()?;
        // The JSON may hold secrets (webhook URLs): only where it broke is logged.
        Ok(json
            .and_then(|j| {
                serde_json::from_str(&j)
                    .inspect_err(|e| {
                        log::error!("stored settings are corrupt ({:?} at {}:{}); keeping what can be read", e.classify(), e.line(), e.column())
                    })
                    .ok()
                    .or_else(|| readable_settings(&j))
            })
            .unwrap_or_default())
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        let json = serde_json::to_string(s).map_err(|e| AppError::Internal(e.to_string()))?;
        self.conn().execute("INSERT OR REPLACE INTO settings (key, value) VALUES ('settings', ?1)", [json])?;
        Ok(())
    }

    /// A value kept beside the settings under its own key (not `settings`), e.g. a vault blob.
    pub fn setting_value(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn().query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    /// Sets (Some) or removes (None) a value kept beside the settings.
    pub fn set_setting_value(&self, key: &str, value: Option<&str>) -> Result<()> {
        match value {
            Some(v) => self.conn().execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)", params![key, v])?,
            None => self.conn().execute("DELETE FROM settings WHERE key = ?1", [key])?,
        };
        Ok(())
    }

    /// Favourites in order:`(id, name, encrypted target, account ids, in tray, sort order, last used)`.
    pub fn favourites(&self) -> Result<Vec<FavouriteRow>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT id, name, target, account_ids, in_tray, sort_order, last_used_at FROM favourites ORDER BY sort_order, id",
        )?;
        let rows = st.query_map([], |r| {
            let (id, ids): (i64, String) = (r.get(0)?, r.get(3)?);
            Ok(FavouriteRow {
                id,
                name: r.get(1)?,
                target: r.get(2)?,
                account_ids: serde_json::from_str(&ids)
                    .inspect_err(|e| log::warn!("favourite {id}: corrupt account ids ({e}); using none"))
                    .unwrap_or_default(),
                in_tray: r.get(4)?,
                sort_order: r.get(5)?,
                last_used_at: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Insert (`id: None`, goes last) or update; the name is trimmed, 1..=60 chars.
    pub fn save_favourite(&self, id: Option<i64>, name: &str, target: &[u8], account_ids: &[i64], in_tray: bool) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 60 {
            return Err(AppError::Invalid(tr!("Favourite names need 1 to 60 characters.")));
        }
        let ids = serde_json::to_string(account_ids).map_err(|e| AppError::Internal(e.to_string()))?;
        let c = self.conn();
        match id {
            Some(id) => {
                let n = c.execute(
                    "UPDATE favourites SET name = ?2, target = ?3, account_ids = ?4, in_tray = ?5 WHERE id = ?1",
                    params![id, name, target, ids, in_tray],
                )?;
                if n == 0 {
                    return Err(AppError::NotFound);
                }
                Ok(id)
            }
            None => Ok(c.query_row(
                "INSERT INTO favourites (name, target, account_ids, in_tray, sort_order) \
                 VALUES (?1, ?2, ?3, ?4, (SELECT COALESCE(MAX(sort_order) + 1, 0) FROM favourites)) RETURNING id",
                params![name, target, ids, in_tray],
                |r| r.get(0),
            )?),
        }
    }

    pub fn remove_favourite(&self, id: i64) -> Result<()> {
        self.conn().execute("DELETE FROM favourites WHERE id = ?1", [id])?;
        Ok(())
    }

    /// `ids` get sort_order 0..n; others keep their order after them.
    pub fn reorder_favourites(&self, ids: &[i64]) -> Result<()> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        let rest: Vec<i64> = {
            let mut st = tx.prepare("SELECT id FROM favourites ORDER BY sort_order, id")?;
            let all = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<i64>>>()?;
            all.into_iter().filter(|id| !ids.contains(id)).collect()
        };
        for (i, id) in ids.iter().chain(rest.iter()).enumerate() {
            tx.execute("UPDATE favourites SET sort_order = ?2 WHERE id = ?1", params![id, i as i64])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn touch_favourite(&self, id: i64, at: i64) -> Result<()> {
        self.conn().execute("UPDATE favourites SET last_used_at = ?2 WHERE id = ?1", params![id, at])?;
        Ok(())
    }

    /// Records a server the account's client reached, unless it was already the latest; only its
    /// newest `MAX_SERVERS` are kept.
    pub fn add_server(&self, account_id: i64, v: &ServerVisit) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let latest: Option<String> = tx
            .query_row("SELECT job_id FROM server_history WHERE account_id = ?1 ORDER BY id DESC LIMIT 1", [account_id], |r| r.get(0))
            .optional()?;
        if latest.as_deref() != Some(&v.job_id) {
            tx.execute(
                "INSERT INTO server_history (account_id, place_id, job_id, private, joined_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![account_id, v.place_id, v.job_id, v.private, v.joined_at],
            )?;
            tx.execute(
                "DELETE FROM server_history WHERE account_id = ?1 AND id NOT IN \
                 (SELECT id FROM server_history WHERE account_id = ?1 ORDER BY id DESC LIMIT ?2)",
                params![account_id, MAX_SERVERS],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The account's servers, newest first.
    pub fn servers(&self, account_id: i64) -> Result<Vec<ServerVisit>> {
        let c = self.conn();
        let mut st = c.prepare("SELECT place_id, job_id, private, joined_at FROM server_history WHERE account_id = ?1 ORDER BY id DESC")?;
        let rows = st.query_map([account_id], |r| Ok(ServerVisit { place_id: r.get(0)?, job_id: r.get(1)?, private: r.get(2)?, joined_at: r.get(3)? }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Records a client that ended badly; only the newest 200 are kept.
    pub fn add_crash(&self, c: &NewCrash) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO crashes (instance_id, account_id, kind, at, uptime_ms, reason, place_id, log_path) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![c.instance_id, c.account_id, c.kind.as_str(), c.at, c.uptime_ms, c.reason, c.place_id, c.log_path],
        )?;
        tx.execute("DELETE FROM crashes WHERE id NOT IN (SELECT id FROM crashes ORDER BY id DESC LIMIT ?1)", [MAX_CRASHES])?;
        tx.commit()?;
        Ok(())
    }

    /// Newest first.
    pub fn crashes(&self) -> Result<Vec<Crash>> {
        let c = self.conn();
        let mut st = c.prepare(
            "SELECT id, instance_id, account_id, kind, at, uptime_ms, reason, place_id, log_path IS NOT NULL \
             FROM crashes ORDER BY id DESC",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Crash {
                id: r.get(0)?,
                instance_id: r.get(1)?,
                account_id: r.get(2)?,
                kind: {
                    let s: String = r.get(3)?;
                    let kind = CrashKind::parse(&s);
                    if kind.as_str() != s {
                        log::debug!("unknown crash kind {s:?}");
                    }
                    kind
                },
                at: r.get(4)?,
                uptime_ms: r.get(5)?,
                reason: r.get(6)?,
                place_id: r.get(7)?,
                has_log: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The client log of a recorded crash (`NotFound` when there is none).
    pub fn crash_log(&self, id: i64) -> Result<String> {
        let path: Option<String> = self.conn().query_row("SELECT log_path FROM crashes WHERE id = ?1", [id], |r| r.get(0))?;
        path.ok_or(AppError::NotFound)
    }

    /// A cached API answer no older than `max_age_ms`, with when it was fetched (unix ms).
    pub fn cache_get(&self, key: &str, max_age_ms: i64, now: i64) -> Result<Option<(String, i64)>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT value, fetched_at FROM cache WHERE key = ?1 AND fetched_at >= ?2",
                params![key, now - max_age_ms],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    pub fn cache_put(&self, key: &str, value: &str, fetched_at: i64) -> Result<()> {
        self.conn().execute(
            "INSERT OR REPLACE INTO cache (key, value, fetched_at) VALUES (?1, ?2, ?3)",
            params![key, value, fetched_at],
        )?;
        Ok(())
    }

    /// Replaces the saved tracked clients with `rows` (instance id, JSON).
    pub fn save_tracked(&self, rows: &[(String, String)]) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM tracked", [])?;
        for (id, data) in rows {
            tx.execute("INSERT INTO tracked (instance_id, data) VALUES (?1, ?2)", params![id, data])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The saved tracked clients (JSON each).
    pub fn tracked(&self) -> Result<Vec<String>> {
        let c = self.conn();
        let mut st = c.prepare("SELECT data FROM tracked")?;
        let rows = st.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("migrations/001_init.sql")),
        M::up(include_str!("migrations/002_account_options.sql")),
        M::up(include_str!("migrations/003_crashes.sql")),
        M::up(include_str!("migrations/004_discord_mute.sql")),
        M::up(include_str!("migrations/005_fps_cap.sql")),
        M::up(include_str!("migrations/006_tracked.sql")),
        M::up(include_str!("migrations/007_full_quality.sql")),
        M::up(include_str!("migrations/008_server_history.sql")),
        M::up(include_str!("migrations/009_vault_recovery.sql")),
    ])
}

fn write_recovery(c: &Connection, r: &RecoveryRecord) -> Result<()> {
    c.execute(
        "INSERT OR REPLACE INTO vault_recovery (id, wrapped_key, kdf_salt, kdf_m_kib, kdf_t, kdf_p) \
         VALUES (1, ?1, ?2, ?3, ?4, ?5)",
        params![r.wrapped_key, r.kdf.salt, r.kdf.m_kib, r.kdf.t, r.kdf.p],
    )?;
    Ok(())
}

/// The previous wrap of the data key must not survive in the WAL: it would open the vault
/// without the new (or any) master password, or with a revoked code.
fn flush_old_wraps(c: &Connection) -> Result<()> {
    // Committed by now: a failure here must not be reported as a failed save. The next
    // checkpoint removes the old wrap.
    match c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get::<_, i64>(0)) {
        Ok(0) => {}
        Ok(_) => log::warn!("database journal not flushed after a vault change: busy"),
        Err(e) => log::warn!("database journal not flushed after a vault change: {e}"),
    }
    Ok(())
}

fn account_row(r: &Row) -> rusqlite::Result<AccountSummary> {
    let groups: Option<String> = r.get(9)?;
    Ok(AccountSummary {
        id: r.get(0)?,
        user_id: r.get(1)?,
        username: r.get(2)?,
        display_name: r.get(3)?,
        alias: r.get(4)?,
        notes: r.get(5)?,
        color: r.get(6)?,
        favourite: r.get(7)?,
        sort_order: r.get(8)?,
        group_ids: groups.map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default(),
        status: {
            let s: String = r.get(10)?;
            let status = AccountStatus::parse(&s);
            if status.as_str() != s {
                log::debug!("unknown account status {s:?}");
            }
            status
        },
        status_checked_at: r.get(11)?,
        created_at: r.get(12)?,
        allow_disconnect: r.get(13)?,
        launch_args: r.get(14)?,
        discord_muted: r.get(15)?,
        fps_cap: r.get(16)?,
        full_quality: r.get(17)?,
    })
}

fn group_row(r: &Row) -> rusqlite::Result<Group> {
    Ok(Group { id: r.get(0)?, name: r.get(1)?, color: r.get(2)?, sort_order: r.get(3)? })
}

/// Rows changed by a write → `NotFound` when none.
fn found(changed: usize) -> Result<()> {
    if changed == 0 { Err(AppError::NotFound) } else { Ok(()) }
}

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::Invalid(msg.into())
}

/// `None` / `""` → no colour; `#rrggbb` → lower-cased; anything else → `Invalid`.
fn color(c: Option<&str>) -> Result<Option<String>> {
    match c.map(str::trim) {
        None | Some("") => Ok(None),
        Some(c) if c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit()) => {
            Ok(Some(c.to_ascii_lowercase()))
        }
        Some(_) => Err(invalid(tr!("Colour must look like #a1b2c3."))),
    }
}

/// One unreadable value (from a newer version, say) costs that value, not every setting.
fn readable_settings(json: &str) -> Option<Settings> {
    let stored: serde_json::Map<String, serde_json::Value> = serde_json::from_str(json).ok()?;
    let mut good = serde_json::to_value(Settings::default()).ok()?;
    for (k, v) in stored {
        let mut next = good.clone();
        next[&k] = v;
        if serde_json::from_value::<Settings>(next.clone()).is_ok() {
            good = next;
        } else {
            log::warn!("setting {k} unreadable: using its default");
        }
    }
    serde_json::from_value(good).ok()
}

/// The limits on what `write_meta` stores.
pub fn check_meta(m: &AccountMeta) -> Result<()> {
    if m.alias.trim().chars().count() > MAX_ALIAS {
        return Err(invalid(tr!("Alias can be at most {max} characters.", max = MAX_ALIAS)));
    }
    if m.notes.chars().count() > MAX_NOTES {
        return Err(invalid(tr!("Notes can be at most {max} characters.", max = MAX_NOTES)));
    }
    color(m.color.as_deref()).map(|_| ())
}

fn write_meta(c: &Connection, id: i64, m: &AccountMeta) -> Result<()> {
    check_meta(m)?;
    let alias = m.alias.trim();
    let color = color(m.color.as_deref())?;
    let sql = "UPDATE accounts SET alias = ?2, notes = ?3, color = ?4, favourite = ?5, updated_at = ?6 WHERE id = ?1";
    found(c.execute(sql, params![id, alias, m.notes, color, m.favourite, now_ms()])?)
}

/// Caller holds a transaction.
fn replace_groups(c: &Connection, account_id: i64, group_ids: &[i64]) -> Result<()> {
    c.execute("DELETE FROM account_groups WHERE account_id = ?1", [account_id])?;
    for &gid in group_ids {
        let exists: bool = c.query_row("SELECT EXISTS (SELECT 1 FROM groups WHERE id = ?1)", [gid], |r| r.get(0))?;
        if !exists {
            return Err(invalid(tr!("That group no longer exists.")));
        }
        c.execute("INSERT OR IGNORE INTO account_groups (account_id, group_id) VALUES (?1, ?2)", [account_id, gid])?;
    }
    Ok(())
}

/// Validates, then inserts (`id: None`) or updates a group; returns its id.
fn write_group(c: &Connection, id: Option<i64>, name: &str, color_in: Option<&str>) -> Result<i64> {
    let name = name.trim();
    if !(1..=MAX_GROUP_NAME).contains(&name.chars().count()) {
        return Err(invalid(tr!("Group names need 1 to {max} characters.", max = MAX_GROUP_NAME)));
    }
    let color = color(color_in)?;
    // `name` is COLLATE NOCASE, so `=` compares case-insensitively.
    let sql = "SELECT EXISTS (SELECT 1 FROM groups WHERE name = ?1 AND id IS NOT ?2)";
    if c.query_row(sql, params![name, id], |r| r.get::<_, bool>(0))? {
        return Err(invalid(tr!("A group called \"{name}\" already exists.", name = name)));
    }
    match id {
        None => {
            let sql = "INSERT INTO groups (name, color, sort_order) \
                 VALUES (?1, ?2, (SELECT COALESCE(MAX(sort_order) + 1, 0) FROM groups)) RETURNING id";
            Ok(c.query_row(sql, params![name, color], |r| r.get(0))?)
        }
        Some(id) => {
            found(c.execute("UPDATE groups SET name = ?2, color = ?3 WHERE id = ?1", params![id, name, color])?)?;
            Ok(id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_bad_setting_keeps_the_others() {
        let v = readable_settings(r#"{"autoLockMinutes":7,"theme":"neon","launchDelayMs":9000}"#).unwrap();
        assert_eq!((v.auto_lock_minutes, v.launch_delay_ms), (7, 9000));
        assert_eq!(v.theme, Settings::default().theme);
    }

    #[test]
    fn a_removed_accounts_id_is_never_used_again() {
        let db = Db::open_in_memory().unwrap();
        let add = |user: i64| {
            db.upsert_account(&NewAccount { user_id: user, username: "u", display_name: "d", cookie: b"x", status: AccountStatus::Unknown, checked_at: None }).unwrap()
        };
        let (a, b) = (add(1), add(2));
        let f = db.save_favourite(None, "f", b"t", &[a, b], false).unwrap();
        db.remove_accounts(&[b]).unwrap();
        assert_eq!(db.favourites().unwrap().iter().find(|x| x.id == f).unwrap().account_ids, [a]);
        let c = add(3);
        assert!(c > b, "{c} after {b}");
        // The same user added again is new too, and an update keeps its id.
        assert_eq!(add(3), c);
    }

    fn add(db: &Db, user_id: i64) -> i64 {
        let name = format!("user{user_id}");
        db.upsert_account(&NewAccount {
            user_id,
            username: &name,
            display_name: &name,
            cookie: b"blob",
            status: AccountStatus::Unknown,
            checked_at: None,
        })
        .unwrap()
    }

    fn group(db: &Db, name: &str) -> i64 {
        db.save_group(&GroupInput { id: None, name: name.into(), color: None }).unwrap().id
    }

    fn order(db: &Db) -> Vec<i64> {
        db.accounts().unwrap().iter().map(|a| a.id).collect()
    }

    fn patch(f: fn(&mut AccountPatch)) -> AccountPatch {
        let mut p = AccountPatch::default();
        f(&mut p);
        p
    }

    fn is_invalid<T>(r: Result<T>) -> bool {
        matches!(r, Err(AppError::Invalid(_)))
    }

    #[test]
    fn opens_migrated_in_memory() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.accounts().unwrap().is_empty());
        assert!(db.groups().unwrap().is_empty());
        assert!(db.vault_record().unwrap().is_none());
    }

    #[test]
    fn opens_file_in_wal_and_reopens() {
        let dir = std::env::temp_dir().join(format!("roalt-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("nested").join("roalt.db");
        {
            let db = Db::open(&path).unwrap();
            add(&db, 1);
            let mode: String = db.conn().query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
            assert_eq!(mode, "wal");
        }
        assert_eq!(Db::open(&path).unwrap().accounts().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_itself_while_open() {
        let dir = std::env::temp_dir().join(format!("roalt-test-{}", uuid::Uuid::new_v4()));
        let db = Db::open(&dir.join("roalt.db")).unwrap();
        add(&db, 1);
        add(&db, 2);
        let copy = dir.join("moved").join("roalt.db");
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        db.copy_to(&copy, || Ok(())).unwrap();
        // The original keeps working; the copy has what it had then.
        add(&db, 3);
        let users = |db: &Db| db.accounts().unwrap().iter().map(|a| a.user_id).collect::<Vec<_>>();
        assert_eq!(users(&Db::open(&copy).unwrap()), [1, 2]);
        assert_eq!(users(&db), [1, 2, 3]);
        // Never over an existing database.
        assert!(db.copy_to(&copy, || Ok(())).is_err());
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replaced_key_wrap_leaves_no_trace_on_disk() {
        let dir = std::env::temp_dir().join(format!("roalt-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("roalt.db");
        let old = b"OLD-DPAPI-WRAP-MARKER-0123456789".to_vec();
        let db = Db::open(&path).unwrap();
        db.save_vault_record(&VaultRecord { mode: VaultMode::Dpapi, wrapped_key: old.clone(), kdf: None }).unwrap();
        add(&db, 1);
        let master = VaultRecord {
            mode: VaultMode::Master,
            wrapped_key: vec![9; 64],
            kdf: Some(Kdf { salt: vec![7; 16], m_kib: 65536, t: 3, p: 4 }),
        };
        db.save_vault_record(&master).unwrap();
        // The process may exit without closing the connection: check the files as they are.
        for f in ["roalt.db", "roalt.db-wal"] {
            let bytes = std::fs::read(dir.join(f)).unwrap_or_default();
            assert!(!bytes.windows(old.len()).any(|w| w == old.as_slice()), "old wrap left in {f}");
        }
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn master_record(wrap: u8) -> VaultRecord {
        VaultRecord {
            mode: VaultMode::Master,
            wrapped_key: vec![wrap; 64],
            kdf: Some(Kdf { salt: vec![7; 16], m_kib: 65536, t: 3, p: 4 }),
        }
    }

    fn recovery_record(wrap: u8) -> RecoveryRecord {
        RecoveryRecord { wrapped_key: vec![wrap; 64], kdf: Kdf { salt: vec![8; 16], m_kib: 262144, t: 3, p: 4 } }
    }

    #[test]
    fn recovery_record_outlives_a_password_change_but_not_master_mode() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.recovery_record().unwrap(), None);
        db.save_vault_record(&master_record(1)).unwrap();

        db.save_recovery_record(&recovery_record(2)).unwrap();
        assert_eq!(db.recovery_record().unwrap(), Some(recovery_record(2)));
        db.save_recovery_record(&recovery_record(3)).unwrap();
        assert_eq!(db.recovery_record().unwrap(), Some(recovery_record(3)), "one code at a time");

        db.save_vault_record(&master_record(4)).unwrap();
        assert_eq!(db.recovery_record().unwrap(), Some(recovery_record(3)), "a password change keeps it");

        db.save_vault_and_recovery(&master_record(5), &recovery_record(6)).unwrap();
        assert_eq!(db.vault_record().unwrap(), Some(master_record(5)));
        assert_eq!(db.recovery_record().unwrap(), Some(recovery_record(6)));

        db.save_vault_record(&VaultRecord { mode: VaultMode::Dpapi, wrapped_key: vec![9; 64], kdf: None }).unwrap();
        assert_eq!(db.recovery_record().unwrap(), None, "leaving master mode deletes it");
    }

    #[test]
    fn saving_vault_and_recovery_is_all_or_nothing() {
        let db = Db::open_in_memory().unwrap();
        db.save_vault_record(&master_record(1)).unwrap();
        db.save_recovery_record(&recovery_record(2)).unwrap();
        db.conn().execute_batch("CREATE TRIGGER no_recovery BEFORE INSERT ON vault_recovery BEGIN SELECT RAISE(ABORT, 'no'); END;").unwrap();
        assert!(db.save_vault_and_recovery(&master_record(3), &recovery_record(4)).is_err());
        assert_eq!(db.vault_record().unwrap(), Some(master_record(1)), "the old password still opens it");
        assert_eq!(db.recovery_record().unwrap(), Some(recovery_record(2)), "and the old code");
    }

    #[test]
    fn migration_keeps_an_existing_vault() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrations().to_version(&mut conn, 8).unwrap();
        conn.execute(
            "INSERT INTO vault (id, mode, wrapped_key, kdf_salt, kdf_m_kib, kdf_t, kdf_p) VALUES (1, 'master', ?1, ?2, 65536, 3, 4)",
            params![vec![1u8; 64], vec![7u8; 16]],
        )
        .unwrap();
        let db = Db::init(conn, None).unwrap();
        assert_eq!(db.vault_record().unwrap(), Some(master_record(1)));
        assert_eq!(db.recovery_record().unwrap(), None);
    }

    #[test]
    fn replaced_recovery_wrap_leaves_no_trace_on_disk() {
        let dir = std::env::temp_dir().join(format!("roalt-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("roalt.db");
        let db = Db::open(&path).unwrap();
        db.save_vault_record(&master_record(1)).unwrap();
        let old = b"OLD-RECOVERY-WRAP-MARKER-0123456".to_vec();
        db.save_recovery_record(&RecoveryRecord { wrapped_key: old.clone(), ..recovery_record(2) }).unwrap();
        add(&db, 1);
        db.save_recovery_record(&recovery_record(3)).unwrap();
        for f in ["roalt.db", "roalt.db-wal"] {
            let bytes = std::fs::read(dir.join(f)).unwrap_or_default();
            assert!(!bytes.windows(old.len()).any(|w| w == old.as_slice()), "old wrap left in {f}");
        }
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_updates_and_keeps_user_details() {
        let db = Db::open_in_memory().unwrap();
        let other = add(&db, 1);
        let id = add(&db, 2);
        let (g1, g2) = (group(&db, "Alts"), group(&db, "Main"));
        let p = AccountPatch {
            alias: Some(" farm ".into()),
            notes: Some("n".into()),
            color: Some("#A1B2C3".into()),
            favourite: Some(true),
            group_ids: Some(vec![g2, g1]),
            allow_disconnect: Some(true),
            launch_args: Some(" --fast ".into()),
            discord_muted: Some(true),
            fps_cap: None,
            full_quality: None,
        };

        db.update_account(id, &p).unwrap();
        db.reorder_accounts(&[id, other]).unwrap();

        let new = NewAccount {
            user_id: 2,
            username: "renamed",
            display_name: "Shown",
            cookie: b"new",
            status: AccountStatus::Valid,
            checked_at: Some(5),
        };
        assert_eq!(db.upsert_account(&new).unwrap(), id);
        let a = db.account(id).unwrap();
        assert_eq!((a.username.as_str(), a.display_name.as_str()), ("renamed", "Shown"));
        assert_eq!((a.status, a.status_checked_at), (AccountStatus::Valid, Some(5)));
        assert_eq!((a.alias.as_str(), a.notes.as_str()), ("farm", "n"));
        assert_eq!(a.color.as_deref(), Some("#a1b2c3"));
        assert!(a.favourite && a.allow_disconnect && a.discord_muted);
        assert_eq!(a.launch_args.as_deref(), Some("--fast"));
        assert_eq!(a.group_ids, vec![g1, g2]);
        assert_eq!(a.sort_order, 0);
        assert_eq!(db.account_cookie(id).unwrap(), (2, b"new".to_vec()));
        assert_eq!(db.account_id_for_user(2).unwrap(), Some(id));
        assert_eq!(db.account_id_for_user(99).unwrap(), None);
        assert_eq!(db.accounts().unwrap().len(), 2);
    }

    #[test]
    fn new_accounts_go_last() {
        let db = Db::open_in_memory().unwrap();
        let (a, b) = (add(&db, 1), add(&db, 2));
        db.reorder_accounts(&[b, a]).unwrap();
        let c = add(&db, 3);
        assert_eq!(order(&db), vec![b, a, c]);
        assert_eq!(db.account(c).unwrap().sort_order, 2);
    }

    #[test]
    fn reorder_with_partial_list() {
        let db = Db::open_in_memory().unwrap();
        let ids: Vec<i64> = (1..=4).map(|u| add(&db, u)).collect();
        db.reorder_accounts(&[ids[2], 999, ids[0], ids[2]]).unwrap();
        assert_eq!(order(&db), vec![ids[2], ids[0], ids[1], ids[3]]);
        let sorts: Vec<i64> = db.accounts().unwrap().iter().map(|a| a.sort_order).collect();
        assert_eq!(sorts, vec![0, 1, 2, 3]);
    }

    #[test]
    fn remove_cascades_groups() {
        let db = Db::open_in_memory().unwrap();
        let (a, b) = (add(&db, 1), add(&db, 2));
        let g = group(&db, "G");
        db.set_account_groups(a, &[g]).unwrap();
        db.set_account_groups(b, &[g]).unwrap();
        let (ua, ub) = (db.account(a).unwrap().user_id, db.account(b).unwrap().user_id);
        for k in [format!("info:{ua}"), format!("headshot:{ua}"), format!("info:{ub}")] {
            db.cache_put(&k, "{}", 1).unwrap();
        }
        db.remove_accounts(&[a, 404]).unwrap();
        assert_eq!(db.cache_get(&format!("info:{ua}"), i64::MAX / 2, 1).unwrap(), None);
        assert_eq!(db.cache_get(&format!("headshot:{ua}"), i64::MAX / 2, 1).unwrap(), None);
        assert!(db.cache_get(&format!("info:{ub}"), i64::MAX / 2, 1).unwrap().is_some());
        assert_eq!(order(&db), vec![b]);
        let links: i64 = db.conn().query_row("SELECT COUNT(*) FROM account_groups", [], |r| r.get(0)).unwrap();
        assert_eq!(links, 1);
        db.remove_group(g).unwrap();
        assert!(db.account(b).unwrap().group_ids.is_empty());
        assert!(matches!(db.account(a), Err(AppError::NotFound)));
    }

    #[test]
    fn update_account_validates() {
        let db = Db::open_in_memory().unwrap();
        let id = add(&db, 1);
        assert!(is_invalid(db.update_account(id, &patch(|p| p.color = Some("red".into())))));
        assert!(is_invalid(db.update_account(id, &patch(|p| p.color = Some("#12345g".into())))));
        assert!(is_invalid(db.update_account(id, &patch(|p| p.alias = Some("x".repeat(65))))));
        assert!(is_invalid(db.update_account(id, &patch(|p| p.notes = Some("x".repeat(2001))))));
        assert!(is_invalid(db.update_account(id, &patch(|p| p.group_ids = Some(vec![77])))));
        let err = db.update_account(id, &patch(|p| p.color = Some("red".into()))).unwrap_err();
        assert_eq!(err.to_string(), "Colour must look like #a1b2c3.");
        assert!(matches!(db.update_account(404, &AccountPatch::default()), Err(AppError::NotFound)));

        // A rejected patch writes nothing, even the valid fields in it.
        let mixed = AccountPatch { alias: Some("kept?".into()), group_ids: Some(vec![77]), ..Default::default() };
        assert!(is_invalid(db.update_account(id, &mixed)));
        assert_eq!(db.account(id).unwrap().alias, "");

        db.update_account(id, &patch(|p| p.alias = Some("é".repeat(64)))).unwrap();
        db.update_account(id, &patch(|p| p.color = Some("#00ff00".into()))).unwrap();
        assert_eq!(db.account(id).unwrap().color.as_deref(), Some("#00ff00"));
        db.update_account(id, &patch(|p| p.color = Some(String::new()))).unwrap();
        let a = db.account(id).unwrap();
        assert_eq!(a.color, None);
        assert_eq!(a.alias.chars().count(), 64);
    }

    #[test]
    fn groups_unique_and_rename() {
        let db = Db::open_in_memory().unwrap();
        let input = |id: Option<i64>, name: &str, color: Option<&str>| GroupInput {
            id,
            name: name.into(),
            color: color.map(Into::into),
        };
        let g = db.save_group(&input(None, "  PvP ", Some("#ABCDEF"))).unwrap();
        assert_eq!((g.name.as_str(), g.color.as_deref(), g.sort_order), ("PvP", Some("#abcdef"), 0));
        assert!(is_invalid(db.save_group(&input(None, "pvp", None))));
        assert!(is_invalid(db.save_group(&input(None, "   ", None))));
        assert!(is_invalid(db.save_group(&input(None, &"x".repeat(41), None))));
        assert!(is_invalid(db.save_group(&input(None, "x", Some("#fff")))));

        let other = group(&db, "Farm");
        let renamed = db.save_group(&input(Some(g.id), "PVP", None)).unwrap();
        assert_eq!((renamed.id, renamed.name.as_str(), renamed.color), (g.id, "PVP", None));
        assert!(is_invalid(db.save_group(&input(Some(other), "pvp", None))));
        assert!(matches!(db.save_group(&input(Some(404), "New", None)), Err(AppError::NotFound)));
        let names: Vec<String> = db.groups().unwrap().into_iter().map(|g| g.name).collect();
        assert_eq!(names, vec!["PVP", "Farm"]);
    }

    #[test]
    fn group_by_name_or_create_is_idempotent() {
        let db = Db::open_in_memory().unwrap();
        let a = db.group_by_name_or_create("Alts", Some("#123456")).unwrap();
        assert_eq!(db.group_by_name_or_create(" alts ", None).unwrap(), a);
        assert_eq!(db.groups().unwrap().len(), 1);
        assert_ne!(db.group_by_name_or_create("Mains", None).unwrap(), a);
    }

    #[test]
    fn stored_settings_with_park_minimized_still_load() {
        let db = Db::open_in_memory().unwrap();
        db.save_settings(&Settings { auto_lock_minutes: 3, ..Settings::default() }).unwrap();
        db.conn().execute("UPDATE settings SET value = json_set(value, '$.parkMinimized', 1) WHERE key = 'settings'", []).unwrap();
        let raw: String = db.conn().query_row("SELECT value FROM settings WHERE key = 'settings'", [], |r| r.get(0)).unwrap();
        assert!(raw.contains("parkMinimized"));
        assert_eq!(db.settings().unwrap().auto_lock_minutes, 3);
    }

    #[test]
    fn settings_default_and_round_trip() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.settings().unwrap(), Settings::default());
        let s = Settings { auto_lock_minutes: 3, clipboard_clear_seconds: 9, ..Settings::default() };
        db.save_settings(&s).unwrap();
        db.save_settings(&s).unwrap();
        assert_eq!(db.settings().unwrap(), s);
        db.conn().execute("UPDATE settings SET value = 'nope' WHERE key = 'settings'", []).unwrap();
        assert_eq!(db.settings().unwrap(), Settings::default());
        // Settings saved before the web dashboard was removed still load.
        let old = r#"{"autoLockMinutes":3,"remote":{"enabled":true}}"#;
        db.conn().execute("UPDATE settings SET value = ?1 WHERE key = 'settings'", [old]).unwrap();
        assert_eq!(db.settings().unwrap().auto_lock_minutes, 3);

        // The guard goes off once for settings saved with it on; later choices stay.
        let on = Settings { allow_public_networks: false, ..Settings::default() };
        let db2 = Db::open_in_memory().unwrap();
        db2.set_setting_value(PUBLIC_GUARD_OFF_ONCE, None).unwrap();
        db2.save_settings(&on).unwrap();
        db2.public_guard_off_once().unwrap();
        assert!(db2.settings().unwrap().allow_public_networks, "off once");
        db2.save_settings(&on).unwrap();
        db2.public_guard_off_once().unwrap();
        assert!(!db2.settings().unwrap().allow_public_networks, "the user's later choice stays");
        assert!(Settings::default().allow_public_networks);

        // The background's cover comes down once, and a later choice stays.
        let heavy = Settings { background_dim: 90, ..Settings::default() };
        db2.set_setting_value(BACKGROUND_COVER_ONCE, None).unwrap();
        db2.save_settings(&heavy).unwrap();
        db2.background_cover_once().unwrap();
        assert_eq!(db2.settings().unwrap().background_dim, Settings::default().background_dim);
        db2.save_settings(&heavy).unwrap();
        db2.background_cover_once().unwrap();
        assert_eq!(db2.settings().unwrap().background_dim, 90, "the user's later choice stays");

        // Values kept beside the settings leave them alone.
        assert_eq!(db.setting_value("discord_webhook").unwrap(), None);
        db.set_setting_value("discord_webhook", Some("00ff")).unwrap();
        db.save_settings(&s).unwrap();
        assert_eq!((db.setting_value("discord_webhook").unwrap().as_deref(), db.settings().unwrap()), (Some("00ff"), s));
        db.set_setting_value("discord_webhook", None).unwrap();
        assert_eq!(db.setting_value("discord_webhook").unwrap(), None);
    }

    #[test]
    fn server_history_keeps_the_newest_per_account() {
        let db = Db::open_in_memory().unwrap();
        let id = add(&db, 1);
        let visit = |job: &str, at| ServerVisit { place_id: 7, job_id: job.into(), private: false, joined_at: at };
        db.add_server(id, &visit("a", 1)).unwrap();
        db.add_server(id, &visit("a", 2)).unwrap();
        assert_eq!(db.servers(id).unwrap().len(), 1, "the same server again isn't a new visit");
        for i in 0..MAX_SERVERS + 5 {
            db.add_server(id, &visit(&format!("job{i}"), i)).unwrap();
        }
        let all = db.servers(id).unwrap();
        assert_eq!(all.len() as i64, MAX_SERVERS);
        assert_eq!(all[0].job_id, format!("job{}", MAX_SERVERS + 4), "newest first");
    }

    #[test]
    fn full_quality_is_kept() {
        let db = Db::open_in_memory().unwrap();
        let id = add(&db, 1);
        assert!(!db.account(id).unwrap().full_quality);
        db.update_account(id, &patch(|p| p.full_quality = Some(true))).unwrap();
        assert!(db.account(id).unwrap().full_quality);
        assert_eq!(db.full_quality_accounts().unwrap(), [id]);
    }

    #[test]
    fn account_options_and_crashes() {
        let db = Db::open_in_memory().unwrap();
        let id = add(&db, 1);
        let a = db.account(id).unwrap();
        assert_eq!((a.allow_disconnect, a.launch_args), (false, None));
        db.update_account(id, &patch(|p| p.launch_args = Some("-x".into()))).unwrap();
        db.update_account(id, &patch(|p| p.launch_args = Some("  ".into()))).unwrap();
        assert_eq!(db.account(id).unwrap().launch_args, None);
        db.update_account(id, &patch(|p| p.fps_cap = Some(15))).unwrap();
        assert_eq!(db.account(id).unwrap().fps_cap, Some(15));
        db.update_account(id, &patch(|p| p.fps_cap = Some(0))).unwrap();
        assert_eq!(db.account(id).unwrap().fps_cap, None);


        let log = r"C:\logs\x.log";
        for n in 0..205 {
            db.add_crash(&NewCrash {
                instance_id: "i",
                account_id: id,
                kind: if n % 2 == 0 { CrashKind::Hang } else { CrashKind::Disconnect },
                at: n,
                uptime_ms: 5,
                reason: Some("Reason: 267"),
                place_id: None,
                log_path: (n == 204).then_some(log),
            })
            .unwrap();
        }
        let list = db.crashes().unwrap();
        assert_eq!(list.len(), 200);
        assert_eq!((list[0].at, list[0].kind, list[0].has_log), (204, CrashKind::Hang, true));
        assert_eq!((list[1].kind, list[1].has_log, list[199].at), (CrashKind::Disconnect, false, 5));
        assert_eq!(db.crash_log(list[0].id).unwrap(), log);
        assert!(matches!(db.crash_log(list[1].id), Err(AppError::NotFound)));
        // They go with the account.
        db.remove_accounts(&[id]).unwrap();
        assert!(db.crashes().unwrap().is_empty());
    }

    #[test]
    fn tracked_clients_are_replaced_wholesale() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.tracked().unwrap().is_empty());
        let row = |id: &str| (id.to_owned(), format!("{{\"id\":\"{id}\"}}"));
        db.save_tracked(&[row("a"), row("b")]).unwrap();
        let mut got = db.tracked().unwrap();
        got.sort();
        assert_eq!(got, [row("a").1, row("b").1]);
        // "a" ended.
        db.save_tracked(&[row("b")]).unwrap();
        assert_eq!(db.tracked().unwrap(), [row("b").1]);
        db.save_tracked(&[]).unwrap();
        assert!(db.tracked().unwrap().is_empty());
    }

    #[test]
    fn favourites_crud_and_order() {
        let db = Db::open_in_memory().unwrap();
        let a = db.save_favourite(None, " Sky ", b"t1", &[1, 2], true).unwrap();
        let b = db.save_favourite(None, "VIP", b"t2", &[], false).unwrap();
        assert!(matches!(db.save_favourite(None, "  ", b"x", &[], true), Err(AppError::Invalid(_))));
        assert!(matches!(db.save_favourite(Some(999), "x", b"x", &[], true), Err(AppError::NotFound)));
        let list = db.favourites().unwrap();
        assert_eq!(list.iter().map(|f| (f.id, f.name.as_str())).collect::<Vec<_>>(), vec![(a, "Sky"), (b, "VIP")]);
        assert_eq!((list[0].account_ids.clone(), list[0].in_tray, list[0].target.clone()), (vec![1, 2], true, b"t1".to_vec()));
        db.save_favourite(Some(a), "Sky 2", b"t3", &[3], false).unwrap();
        db.reorder_favourites(&[b]).unwrap();
        db.touch_favourite(a, 42).unwrap();
        let list = db.favourites().unwrap();
        assert_eq!(list.iter().map(|f| f.id).collect::<Vec<_>>(), vec![b, a]);
        assert_eq!((list[1].name.as_str(), list[1].last_used_at), ("Sky 2", Some(42)));
        db.remove_favourite(b).unwrap();
        assert_eq!(db.favourites().unwrap().len(), 1);
    }

    #[test]
    fn vault_record_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let dpapi = VaultRecord { mode: VaultMode::Dpapi, wrapped_key: vec![1, 2, 3], kdf: None };
        db.save_vault_record(&dpapi).unwrap();
        assert_eq!(db.vault_record().unwrap(), Some(dpapi));

        let master = VaultRecord {
            mode: VaultMode::Master,
            wrapped_key: vec![9; 40],
            kdf: Some(Kdf { salt: vec![7; 16], m_kib: 65536, t: 3, p: 4 }),
        };
        db.save_vault_record(&master).unwrap();
        assert_eq!(db.vault_record().unwrap(), Some(master));
        let rows: i64 = db.conn().query_row("SELECT COUNT(*) FROM vault", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 1);

        let back = VaultRecord { mode: VaultMode::Dpapi, wrapped_key: vec![4], kdf: None };
        db.save_vault_record(&back).unwrap();
        let salt: Option<Vec<u8>> = db.conn().query_row("SELECT kdf_salt FROM vault", [], |r| r.get(0)).unwrap();
        assert_eq!(salt, None);
        assert_eq!(db.vault_record().unwrap(), Some(back));
    }

    #[test]
    fn cookie_and_status() {
        let db = Db::open_in_memory().unwrap();
        let id = add(&db, 42);
        assert_eq!(db.account_cookie(id).unwrap(), (42, b"blob".to_vec()));
        db.set_cookie(id, b"rotated").unwrap();
        assert_eq!(db.account_cookie(id).unwrap().1, b"rotated".to_vec());

        db.set_status(id, AccountStatus::Invalid, 10, None, None).unwrap();
        let a = db.account(id).unwrap();
        assert_eq!((a.status, a.status_checked_at, a.username.as_str()), (AccountStatus::Invalid, Some(10), "user42"));
        db.set_status(id, AccountStatus::Valid, 11, Some("new"), Some("New")).unwrap();
        let a = db.account(id).unwrap();
        assert_eq!((a.status, a.username.as_str(), a.display_name.as_str()), (AccountStatus::Valid, "new", "New"));

        assert!(matches!(db.account_cookie(404), Err(AppError::NotFound)));
        assert!(matches!(db.set_cookie(404, b"x"), Err(AppError::NotFound)));
        assert!(matches!(db.set_status(404, AccountStatus::Valid, 1, None, None), Err(AppError::NotFound)));
    }

    #[test]
    fn set_meta_and_groups_for_import() {
        let db = Db::open_in_memory().unwrap();
        let id = add(&db, 1);
        let meta = AccountMeta { alias: "a".into(), notes: "b".into(), color: Some("#000000".into()), favourite: true };
        db.set_meta(id, &meta).unwrap();
        let g = db.group_by_name_or_create("G", None).unwrap();
        db.set_account_groups(id, &[g, g]).unwrap();
        let a = db.account(id).unwrap();
        assert_eq!((a.alias.as_str(), a.notes.as_str(), a.color.as_deref(), a.favourite), ("a", "b", Some("#000000"), true));
        assert_eq!(a.group_ids, vec![g]);
        db.set_account_groups(id, &[]).unwrap();
        assert!(db.account(id).unwrap().group_ids.is_empty());
        assert!(matches!(db.set_meta(404, &meta), Err(AppError::NotFound)));
        assert!(matches!(db.set_account_groups(404, &[g]), Err(AppError::NotFound)));
    }
}
