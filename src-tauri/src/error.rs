use serde::{Serialize, Serializer};
use ts_rs::TS;

/// Every error the UI can see. Messages are user-facing and must never contain a cookie,
/// ticket, link code or anything else from the vault.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("The vault is locked.")]
    Locked,
    #[error("Wrong password.")]
    WrongPassword,
    #[error("This cookie is not valid or has expired. Sign in again.")]
    CookieInvalid,
    #[error("Roblox wants a human check for this account. Sign in to it again with Add account > Sign in with Roblox.")]
    Challenge,
    #[error("Roblox is limiting requests. Try again in {0} s.")]
    RateLimited(u64),
    #[error("Roblox now requires a bound session for this account.")]
    BoundSessionRequired,
    #[error("Roblox answered unexpectedly ({0}).")]
    Roblox(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Not found.")]
    NotFound,
    #[error("Cancelled.")]
    Cancelled,
    #[error("{0}")]
    Invalid(String),
    /// Refused for now, for a reason that may pass by itself (`temporary`); to the UI, `Invalid`.
    #[error("{0}")]
    Wait(String),
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Encryption failed.")]
    Crypto,
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Locked => "locked",
            Self::WrongPassword => "wrongPassword",
            Self::CookieInvalid => "cookieInvalid",
            Self::Challenge => "challenge",
            Self::RateLimited(_) => "rateLimited",
            Self::BoundSessionRequired => "boundSessionRequired",
            Self::Roblox(_) => "roblox",
            Self::Network(_) => "network",
            Self::NotFound => "notFound",
            Self::Cancelled => "cancelled",
            Self::Invalid(_) | Self::Wait(_) => "invalid",
            Self::Storage(_) => "storage",
            Self::Crypto => "crypto",
            Self::Internal(_) => "internal",
        }
    }

    /// May pass by itself (the network, Roblox, a locked vault), so the same thing is worth trying
    /// again later. A bad cookie or a refused target (Roblox's 4xx) stays until someone acts.
    pub fn temporary(&self) -> bool {
        match self {
            Self::Locked | Self::RateLimited(_) | Self::Network(_) | Self::Wait(_) => true,
            Self::Roblox(why) => !why.starts_with("HTTP 4"),
            _ => false,
        }
    }

    /// Roblox answered 403, and not with a human check (`Challenge`) or for a CSRF token, which
    /// the API client handles itself.
    pub fn is_forbidden(&self) -> bool {
        matches!(self, Self::Roblox(why) if why == "HTTP 403")
    }

    /// Logs this error (English, secret-free like every message) at a level fit for its kind.
    pub fn log(&self) {
        let level = match self {
            Self::Cancelled | Self::Invalid(_) | Self::Wait(_) | Self::Locked | Self::WrongPassword | Self::NotFound => log::Level::Debug,
            Self::Internal(_) | Self::Storage(_) | Self::Crypto => log::Level::Error,
            _ => log::Level::Warn,
        };
        log::log!(level, "{} error: {self}", self.kind());
    }

    /// The message in the current language (Display stays English, for logs).
    pub fn message(&self) -> String {
        match self {
            Self::Locked => tr!("The vault is locked."),
            Self::WrongPassword => tr!("Wrong password."),
            Self::CookieInvalid => tr!("This cookie is not valid or has expired. Sign in again."),
            Self::Challenge => {
                tr!("Roblox wants a human check for this account. Sign in to it again with Add account > Sign in with Roblox.")
            }
            Self::RateLimited(s) => tr!("Roblox is limiting requests. Try again in {seconds} s.", seconds = s),
            Self::BoundSessionRequired => tr!("Roblox now requires a bound session for this account."),
            Self::Roblox(why) => tr!("Roblox answered unexpectedly ({why}).", why = why),
            Self::Network(why) => tr!("Network error: {why}", why = why),
            Self::NotFound => tr!("Not found."),
            Self::Cancelled => tr!("Cancelled."),
            Self::Invalid(m) | Self::Wait(m) | Self::Internal(m) => m.clone(),
            Self::Storage(why) => tr!("Storage error: {why}", why = why),
            Self::Crypto => tr!("Encryption failed."),
        }
    }
}

/// What a rejected `invoke` carries to the UI.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct ErrorPayload {
    pub kind: String,
    pub message: String,
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // Only IPC serializes an AppError: every command error the UI gets is logged here once.
        self.log();
        ErrorPayload { kind: self.kind().into(), message: self.message() }.serialize(s)
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::QueryReturnedNoRows => Self::NotFound,
            e => Self::Storage(e.to_string()),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

pub type Result<T, E = AppError> = std::result::Result<T, E>;
