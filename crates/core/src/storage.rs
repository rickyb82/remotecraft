//! Files: where the desk lives and how it is written (user-only, atomically, with the
//! previous version kept as a backup).

use std::path::{Path, PathBuf};

use crate::model::Data;
use crate::{Error, Result, limits};

/// Writes a file only the user can read (0600 on Unix): a temporary file renamed over the old
/// one, so a crash never leaves half a file. The temporary file is created private (never
/// readable by others, not even for an instant), with a name of its own, and flushed to disk
/// before the rename; a directory that has to be created is private too (0700).
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let io = |p: &Path, e: std::io::Error| Error::Io(format!("{}: {e}", p.display()));
    if path.file_name().is_none() {
        return Err(Error::Io(format!("{}: not a file name", path.display())));
    }
    if let Some(dir) = path.parent() {
        create_private_dir(dir).map_err(|e| io(dir, e))?;
    }
    let (file, tmp) = create_tmp(path).map_err(|e| io(path, e))?;
    fill(file, bytes).and_then(|()| std::fs::rename(&tmp, path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(path, e)
    })
}

/// Writes the whole file, flushes it to the disk and closes it (Windows renames only closed
/// files).
fn fill(mut file: std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Creates `dir` and its missing parents, user-only on Unix (directories that exist keep
/// their mode).
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

/// A new, empty, user-only file next to `path` (`<name>.<pid>.<n>.tmp`), created exclusively:
/// never one that already exists, which another user could have opened beforehand.
fn create_tmp(path: &Path) -> std::io::Result<(std::fs::File, PathBuf)> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut last = std::io::Error::from(std::io::ErrorKind::AlreadyExists);
    for _ in 0..4 {
        let tmp = path.with_file_name(format!("{name}.{}.{}.tmp", process_id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&tmp) {
            Ok(f) => return Ok((f, tmp)),
            // Left by a process that crashed with the same id: take the next number.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// This process's id where there is one (the browser has none, and asking there panics).
fn process_id() -> u32 {
    #[cfg(any(unix, windows))]
    let id = std::process::id();
    #[cfg(not(any(unix, windows)))]
    let id = 0;
    id
}

/// Appends a line to a user-only file.
pub fn append_private(path: &Path, line: &str) -> Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        create_private_dir(dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
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
/// (never silently replaced by an empty desk). The caller runs [`Data::repair`] and keeps the
/// problems it reports: repairing here would fix them before anyone could see them.
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
    serde_json::from_value(value).map_err(|e| not_desk(e.to_string()))
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
    fn private_files_are_never_readable_by_others() {
        let d = dir("private");
        let sub = d.join("sub");
        let path = sub.join("tokens.json");
        // A directory created for the file is user-only.
        write_private(&path, b"{\"a\": 1}").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&sub).unwrap().permissions().mode() & 0o777, 0o700);
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // The temporary file is user-only from the moment it exists (it used to be created
        // with the umask, 0644, and chmodded only after the secrets were in it).
        let (f, tmp) = create_tmp(&path).unwrap();
        drop(f);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&tmp).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_file(&tmp).unwrap();
        // Its name is its own: a file planted at the old fixed name (opened by someone else
        // beforehand) is never written into.
        std::fs::write(sub.join("tokens.tmp"), "planted").unwrap();
        write_private(&path, b"{\"a\": 2}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"a\": 2}");
        assert_eq!(std::fs::read_to_string(sub.join("tokens.tmp")).unwrap(), "planted");
        // Nothing left behind next to the file.
        let mut names: Vec<String> = std::fs::read_dir(&sub).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["tokens.json", "tokens.tmp"]);
        assert!(write_private(&d.join(".."), b"x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn load_leaves_the_repair_and_its_problems_to_the_caller() {
        let d = dir("repair");
        let path = d.join("desk.json");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(&path, r#"{"customers":[{"id":1,"name":"X"}],"devices":[{"id":7,"customer_id":1,"rustdesk_id":"123456&calc.exe&rem"}]}"#)
            .unwrap();
        let mut data = load(&path).unwrap();
        assert_eq!(data.repair(), vec!["device 7: invalid RustDesk ID cleared"]);
        assert_eq!(data.devices[0].rustdesk_id, "");
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
