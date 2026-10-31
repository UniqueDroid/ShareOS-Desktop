use crate::nas::{private_file, Nas, Root};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct SavedFile {
    pub root: String,
    pub path: String,
    pub name: String,
    pub object: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub format: u32,
    pub box_url: String,
    pub user: String,
    pub created: u64,
    pub files: Vec<SavedFile>,
    pub skipped_links: usize,
}
pub fn hash(path: &Path) -> Result<(u64, String)> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut size = 0;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
        size += n as u64;
    }
    Ok((size, format!("{:x}", digest.finalize())))
}
// The manifest uses numeric objects, never remote paths as local filenames.
// Only a completed directory containing manifest.json is a valid snapshot.
pub fn create(nas: &Nas, roots: Vec<Root>, destination: &Path) -> Result<String> {
    ensure!(!roots.is_empty(), "Mindestens einen Ordner auswählen");
    fs::create_dir_all(destination)?;
    let id = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let staging = destination.join(format!(".incomplete-{id}"));
    fs::create_dir(&staging)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))?;
    }
    let result = (|| -> Result<String> {
        let mut snapshot = Snapshot {
            format: 1,
            box_url: nas.base.to_string(),
            user: nas.user.clone(),
            created: (id / 1_000_000_000) as u64,
            files: vec![],
            skipped_links: 0,
        };
        for root in roots {
            let mut pending = vec![String::new()];
            while let Some(path) = pending.pop() {
                ensure!(path.matches('/').count() < 128, "Zu große Verzeichnistiefe");
                let listing = nas.list(&root.name, &path)?;
                ensure!(!listing.truncated, "Ordner {}/{path} enthält mehr Einträge als die NAS-API liefert; Sicherung abgebrochen", root.name);
                for entry in listing.entries {
                    ensure!(safe_name(&entry.name), "Ungültiger Dateiname vom NAS");
                    match entry.kind.as_str() {
                        "dir" => pending.push(if path.is_empty() {
                            entry.name
                        } else {
                            format!("{path}/{}", entry.name)
                        }),
                        "file" => {
                            let object = format!("{:08}.data", snapshot.files.len());
                            let target = staging.join(&object);
                            nas.download(&root.name, &path, &entry.name, &target)?;
                            let (bytes, sha256) = hash(&target)?;
                            ensure!(
                                bytes == entry.size,
                                "Dateigröße während der Sicherung geändert: {}",
                                entry.name
                            );
                            snapshot.files.push(SavedFile {
                                root: root.name.clone(),
                                path: path.clone(),
                                name: entry.name,
                                object,
                                bytes,
                                sha256,
                            });
                        }
                        _ => snapshot.skipped_links += 1,
                    }
                }
            }
        }
        let mut file = private_file(&staging.join("manifest.json"))?;
        serde_json::to_writer_pretty(&mut file, &snapshot)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        let completed = destination.join(format!("snapshot-{id}"));
        fs::rename(&staging, &completed)?;
        Ok(format!(
            "Sicherung erstellt: {} · {} Dateien · {} Links/Spezialdateien ausgelassen",
            completed.display(),
            snapshot.files.len(),
            snapshot.skipped_links
        ))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        && !name.ends_with(['.', ' '])
        && !name.chars().any(char::is_control)
        && !matches!(
            name.split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase()
                .as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        )
}
pub fn load(directory: &Path) -> Result<Snapshot> {
    let snapshot: Snapshot =
        serde_json::from_reader(fs::File::open(directory.join("manifest.json"))?)?;
    ensure!(snapshot.format == 1, "Unbekanntes Sicherungsformat");
    Ok(snapshot)
}
pub fn restore(directory: &Path, index: usize, destination: &Path) -> Result<String> {
    let snapshot = load(directory)?;
    let saved = snapshot
        .files
        .get(index)
        .context("Datei fehlt im Sicherungsstand")?;
    ensure!(safe_name(&saved.name), "Ungültiger Dateiname");
    let object = Path::new(&saved.object);
    ensure!(
        object.components().count() == 1
            && matches!(object.components().next(), Some(Component::Normal(_)))
            && safe_name(&saved.object),
        "Ungültiger Sicherungspfad"
    );
    let source = directory.join(object);
    ensure!(
        fs::symlink_metadata(&source)?.file_type().is_file(),
        "Sicherungsobjekt ist keine reguläre Datei"
    );
    ensure!(
        hash(&source)? == (saved.bytes, saved.sha256.clone()),
        "Prüfsumme stimmt nicht – Wiederherstellung abgebrochen"
    );
    let target = destination.join(&saved.name);
    let mut output = private_file(&target)?;
    let result = (|| -> Result<()> {
        std::io::copy(&mut fs::File::open(source)?, &mut output)?;
        output.sync_all()?;
        ensure!(
            hash(&target)? == (saved.bytes, saved.sha256.clone()),
            "Prüfsumme der wiederhergestellten Datei stimmt nicht"
        );
        Ok(())
    })();
    if let Err(error) = result {
        drop(output);
        let _ = fs::remove_file(&target);
        return Err(error);
    }
    Ok(format!("Wiederhergestellt: {}", target.display()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restore_checks_integrity_and_preserves_existing_files() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        fs::write(source.path().join("00000000.data"), b"version one").unwrap();
        let (bytes, sha256) = hash(&source.path().join("00000000.data")).unwrap();
        let mut s = Snapshot {
            format: 1,
            box_url: "https://box/".into(),
            user: "alice".into(),
            created: 0,
            skipped_links: 0,
            files: vec![SavedFile {
                root: "home".into(),
                path: "".into(),
                name: "file.txt".into(),
                object: "00000000.data".into(),
                bytes,
                sha256,
            }],
        };
        let save = |s: &Snapshot| {
            fs::write(
                source.path().join("manifest.json"),
                serde_json::to_vec(s).unwrap(),
            )
            .unwrap()
        };
        save(&s);
        restore(source.path(), 0, dest.path()).unwrap();
        assert!(restore(source.path(), 0, dest.path()).is_err());
        assert_eq!(
            fs::read(dest.path().join("file.txt")).unwrap(),
            b"version one"
        );
        fs::remove_file(dest.path().join("file.txt")).unwrap();
        fs::write(source.path().join("00000000.data"), b"corrupt").unwrap();
        assert!(restore(source.path(), 0, dest.path()).is_err());
        assert!(!dest.path().join("file.txt").exists());
        s.files[0].object = "../outside".into();
        save(&s);
        assert!(restore(source.path(), 0, dest.path()).is_err());
    }
}
