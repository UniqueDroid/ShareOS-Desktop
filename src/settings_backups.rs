//! Settings backups of the box stored on this computer: one folder, one JSON
//! file per backup (as the box exports it). The list reads only the header:
//! box, date and version are readable in encrypted backups too (firmware
//! with backup format 2); older encrypted files fall back to the file date.
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct BackupFile {
    pub path: PathBuf,
    pub name: String,
    pub host: String,
    pub version: String,
    /// seconds since 1970 (from the backup, else the file's modification time)
    pub created: i64,
    pub encrypted: bool,
    /// only known for unencrypted backups (encrypted ones hide their content)
    pub certificate: Option<bool>,
    pub ca: Option<bool>,
}

/// `~/ShareOS-Sicherungen` (Windows: in the user profile).
pub fn default_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join("ShareOS-Sicherungen"))
}

/// The header of a backup file, None for anything else.
pub fn read(path: &Path) -> Option<BackupFile> {
    let meta = std::fs::metadata(path).ok()?;
    // settings backups are small; do not read big unrelated JSON files
    if !meta.is_file() || meta.len() > 16 * 1024 * 1024 {
        return None;
    }
    let value: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let format = value["shareos_backup"].as_u64()?;
    if !(1..=2).contains(&format) {
        return None;
    }
    let encrypted = value["encrypted"].as_bool() == Some(true);
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64);
    let certificate = value["certificate"]["key"].is_string()
        || value["files"]["/etc/ssl/shareos/server.key"].is_string();
    let ca =
        value["ca"]["ca.key"].is_string() || value["files"]["/etc/shareos/ca/ca.key"].is_string();
    Some(BackupFile {
        path: path.to_path_buf(),
        name: path.file_name()?.to_string_lossy().into_owned(),
        host: value["host"].as_str().unwrap_or_default().to_string(),
        version: value["version"].as_str().unwrap_or_default().to_string(),
        created: value["created"]
            .as_i64()
            .filter(|t| *t > 0)
            .unwrap_or(modified),
        encrypted,
        certificate: (!encrypted).then_some(certificate),
        ca: (!encrypted).then_some(ca),
    })
}

/// All backups in the folder, newest first.
pub fn scan(dir: &Path) -> Result<Vec<BackupFile>> {
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut list: Vec<BackupFile> = std::fs::read_dir(dir)
        .with_context(|| format!("Sicherungsordner nicht lesbar: {}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .filter_map(|path| read(&path))
        .collect();
    list.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.name.cmp(&a.name)));
    Ok(list)
}

/// A file name in the folder that does not exist yet.
pub fn free_name(dir: &Path, name: &str) -> PathBuf {
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, "json"));
    let mut path = dir.join(name);
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    path
}

/// UTC offset of this computer, read once at start (before other threads
/// exist; the time crate refuses it later on Unix). Missing: UTC.
pub static LOCAL_OFFSET: std::sync::OnceLock<time::UtcOffset> = std::sync::OnceLock::new();
pub fn init_local_offset() {
    let _ =
        LOCAL_OFFSET.set(time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC));
}

/// "06.11.2026 22:17" in local time.
pub fn date(created: i64) -> String {
    let offset = LOCAL_OFFSET.get().copied().unwrap_or(time::UtcOffset::UTC);
    let format = time::macros::format_description!("[day].[month].[year] [hour]:[minute]");
    time::OffsetDateTime::from_unix_timestamp(created)
        .ok()
        .and_then(|t| t.to_offset(offset).format(&format).ok())
        .unwrap_or_else(|| "–".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lists_backups_newest_first_with_their_header() {
        let dir = std::env::temp_dir().join(format!("sos-backups-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
        write(
            "plain.json",
            r#"{"shareos_backup":2,"host":"nas","version":"0.2.0-beta.21","created":1700000000,"configs":{},"certificate":null}"#,
        );
        write(
            "enc.json",
            r#"{"shareos_backup":2,"encrypted":true,"host":"nas","version":"0.2.0-beta.21","created":1750000000,"data":"x"}"#,
        );
        write(
            "old-enc.json",
            r#"{"shareos_backup":1,"encrypted":true,"data":"x"}"#,
        );
        write(
            "old-plain.json",
            r#"{"shareos_backup":1,"host":"alt","created":1600000000,"files":{"/etc/ssl/shareos/server.key":"k"}}"#,
        );
        write("other.json", r#"{"hello":1}"#);
        write("broken.json", "{");
        write("notes.txt", "x");
        let list = scan(&dir).unwrap();
        let names: Vec<_> = list.iter().map(|b| b.name.as_str()).collect();
        // the old encrypted file has no date: its modification time (now) is newest
        assert_eq!(
            names,
            ["old-enc.json", "enc.json", "plain.json", "old-plain.json"]
        );
        let enc = &list[1];
        assert!(enc.encrypted && enc.host == "nas" && enc.certificate.is_none());
        assert_eq!(list[2].certificate, Some(false));
        assert_eq!(list[3].certificate, Some(true));
        assert_eq!(list[3].ca, Some(false));
        assert_eq!(free_name(&dir, "plain.json"), dir.join("plain (2).json"));
        assert_eq!(free_name(&dir, "new.json"), dir.join("new.json"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn dates_are_readable() {
        // without init_local_offset: UTC
        assert_eq!(date(0), "01.01.1970 00:00");
        assert_eq!(date(1_762_464_000), "06.11.2025 21:20");
    }
}
