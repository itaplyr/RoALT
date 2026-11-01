//! The Windows autopsy report is built from Windows crash dumps, event-log records and
//! Roblox's crash handler. Linux has no equivalent here, so a bad end is still recorded, but
//! without that extra report.

use super::tracker::Ended;

pub fn report(_c: &Ended) {}
