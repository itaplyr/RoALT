//! Mods: packs of content files (textures, sounds, fonts, meshes) that change how Roblox looks and
//! sounds. Roblox's own folder is never changed: `runtime` builds RoALT's own copy of the version
//! folder (hard links, the mods' files laid over) and a launch starts the client from there.
//! `pack` says what a mod may hold and reads and writes its zip; `library` keeps the imported ones.

pub mod library;
pub mod pack;
pub mod runtime;

use std::path::{Path, PathBuf};

/// `%LOCALAPPDATA%\RoALT\mods` (the data folder in a test run), whatever folder the data moved
/// to: on Roblox's drive, so the copy can be hard links.
pub fn root() -> PathBuf {
    std::env::var_os("ROALT_DATA_DIR").map(PathBuf::from).unwrap_or_else(crate::default_dir).join("mods")
}

/// `root()`, made (for this Windows user only) when missing.
fn ensure_root() -> std::io::Result<PathBuf> {
    let root = root();
    if !root.is_dir() {
        std::fs::create_dir_all(&root)?;
        crate::restrict_to_user(&root);
    }
    Ok(root)
}

/// A path inside `root()`: RoALT's own copy, never a Roblox install to build from.
#[allow(dead_code)]
pub fn is_ours(path: &Path) -> bool {
    inside(path, &root())
}

/// `path` is `dir` or under it, ignoring case as Windows does (pure).
fn inside(path: &Path, dir: &Path) -> bool {
    let (p, d) = (path.to_string_lossy().to_lowercase().replace('/', "\\"), dir.to_string_lossy().to_lowercase().replace('/', "\\"));
    let d = d.trim_end_matches('\\');
    p == d || p.strip_prefix(d).is_some_and(|rest| rest.starts_with('\\'))
}

#[cfg(test)]
mod tests {
    #[test]
    fn inside() {
        use std::path::Path;
        let d = Path::new(r"C:\Users\a\AppData\Local\RoALT\mods");
        assert!(super::inside(Path::new(r"c:\users\A\appdata\local\roalt\MODS\runtime\x\RobloxPlayerBeta.exe"), d));
        assert!(!super::inside(Path::new(r"C:\Users\a\AppData\Local\RoALT\mods-old\x.exe"), d));
        assert!(!super::inside(Path::new(r"C:\Users\a\AppData\Local\Roblox\Versions\v\RobloxPlayerBeta.exe"), d));
    }
}
