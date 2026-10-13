//! Files: where the desk lives and how it is written (user-only, atomically, with the
//! previous version kept as a backup).

use std::path::{Path, PathBuf};

use crate::model::Data;
use crate::{Error, Result, limits};

/// Writes a file only the user can read (0600 on Unix): a temporary file renamed over the old
/// one, so a crash never leaves half a file.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| Error::Io(format!("{}: {e}", tmp.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))
}

/// Appends a line to a user-only file.
pub fn append_private(path: &Path, line: &str) -> Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
    writeln!(f, "{line}").map_err(|e| Error::Io(format!("{}: {e}", path.display())))
}

/// Where RemoteCraft keeps its files: `$REMOTECRAFT_HOME`, else the platform data directory
/// (`~/.local/share/remotecraft` on Linux, `~/Library/Application Support/remotecraft` on
/// macOS, `%APPDATA%\remotecraft` on Windows).
pub fn home_dir() -> PathBuf {
    if let Some(h) = std::env::var_os("REMOTECRAFT_HOME").filter(|h| !h.is_empty()) {
        return PathBuf::from(h);
    }
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library").join("Application Support"));
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")));
    base.unwrap_or_else(|| PathBuf::from(".")).join("remotecraft")
}

/// Reads the desk; a missing file is an empty desk. A file that does not parse is an error
/// (never silently replaced by an empty desk).
pub fn load(path: &Path) -> Result<Data> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Data::new()),
        Err(e) => return Err(Error::Io(format!("{}: {e}", path.display()))),
    };
    if meta.len() > limits::MAX_FILE {
        return Err(Error::Io(format!("{}: file too large", path.display())));
    }
    let text = std::fs::read_to_string(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
    let not_desk = |e: String| Error::Io(format!("{}: not a RemoteCraft desk: {e}", path.display()));
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| not_desk(e.to_string()))?;
    if !value.is_object() {
        return Err(not_desk("expected an object".into()));
    }
    let mut data: Data = serde_json::from_value(value).map_err(|e| not_desk(e.to_string()))?;
    data.repair();
    Ok(data)
}

/// Writes the desk, keeping the previous file as `<name>.bak`.
pub fn save(path: &Path, data: &Data) -> Result<()> {
    let text = serde_json::to_string_pretty(data).map_err(|e| Error::Io(e.to_string()))?;
    if path.exists() {
        let _ = std::fs::copy(path, path.with_extension("json.bak"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path.with_extension("json.bak"), std::fs::Permissions::from_mode(0o600));
        }
    }
    write_private(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("remotecraft-storage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn round_trip_with_backup() {
        let d = dir("rt");
        let path = d.join("desk.json");
        assert_eq!(load(&path).unwrap(), Data::new());
        let mut data = Data::new();
        data.preferences.company_name = "Sirius".into();
        save(&path, &data).unwrap();
        save(&path, &data).unwrap();
        assert_eq!(load(&path).unwrap().preferences.company_name, "Sirius");
        assert!(path.with_extension("json.bak").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_broken_file_is_an_error_not_an_empty_desk() {
        let d = dir("broken");
        let path = d.join("desk.json");
        std::fs::create_dir_all(&d).unwrap();
        for junk in ["{", "[]", "\u{0}\u{1}", "{\"customers\": 5}"] {
            std::fs::write(&path, junk).unwrap();
            assert!(load(&path).is_err(), "{junk:?}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn append_keeps_lines() {
        let d = dir("append");
        let path = d.join("audit.jsonl");
        append_private(&path, "a").unwrap();
        append_private(&path, "b").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\nb\n");
        let _ = std::fs::remove_dir_all(&d);
    }
}
