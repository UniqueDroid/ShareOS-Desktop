//! Dropped local files always go to the authenticated user's NAS home.
use crate::{nas::Nas, transfer::Progress};
use anyhow::{ensure, Context, Result};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};
pub const FOLDER: &str = "File Drop";
pub struct Report {
    pub root: String,
    pub uploaded: usize,
    pub failures: Vec<String>,
}
impl Report {
    pub fn summary(&self) -> String {
        let success = format!(
            "File Drop: {} Datei(en) nach Eigene Dateien / {FOLDER} hochgeladen.",
            self.uploaded
        );
        if self.failures.is_empty() {
            success
        } else {
            format!(
                "Fehler: {success} Nicht übertragen: {}",
                self.failures.join("; ")
            )
        }
    }
}
fn local_name(path: &Path) -> Result<&str> {
    ensure!(
        path.is_file(),
        "Nur einzelne Dateien unterstützen File Drop: {}",
        path.display()
    );
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Dateiname ist kein UTF-8")?;
    ensure!(
        !name.is_empty()
            && name.len() <= 255
            && !name.contains('/')
            && !name.chars().any(char::is_control),
        "Dateiname ist für die NAS ungültig"
    );
    Ok(name)
}
fn unique_name(name: &str, existing: &HashSet<String>) -> Result<String> {
    if !existing.contains(name) {
        return Ok(name.into());
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(name);
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_default();
    for number in 2..=9999 {
        let suffix = format!(" ({number}){extension}");
        ensure!(
            suffix.len() < 255,
            "Dateiname kann nicht eindeutig gekürzt werden"
        );
        let mut end = stem.len().min(255 - suffix.len());
        while !stem.is_char_boundary(end) {
            end -= 1;
        }
        let candidate = format!("{}{suffix}", &stem[..end]);
        if !existing.contains(&candidate) {
            return Ok(candidate);
        }
    }
    anyhow::bail!("Zu viele gleichnamige Dateien für {name}")
}
pub fn upload(nas: &Nas, files: Vec<PathBuf>) -> Result<Report> {
    ensure!(!files.is_empty(), "Keine Dateien abgelegt");
    // Reject directories before creating anything on the NAS.
    for path in &files {
        local_name(path)?;
    }
    let home = nas
        .roots()?
        .into_iter()
        .find(|root| root.home)
        .context("Dieses Konto hat keinen eigenen NAS-Ordner für File Drop")?;
    ensure!(!home.ro, "Der eigene NAS-Ordner ist nur lesbar");
    let listing = nas.list(&home.name, "")?;
    match listing.entries.iter().find(|entry| entry.name == FOLDER) {
        Some(entry) => ensure!(
            entry.kind == "dir",
            "Im Home existiert bereits eine Datei oder Verknüpfung namens „File Drop“"
        ),
        None => {
            ensure!(
                !listing.truncated,
                "Die Home-Liste ist unvollständig; File Drop kann nicht sicher angelegt werden"
            );
            nas.mkdir(&home.name, "", FOLDER)
                .context("File-Drop-Ordner konnte nicht angelegt werden")?;
        }
    }
    let contents = nas.list(&home.name, FOLDER)?;
    ensure!(
        !contents.truncated,
        "File Drop enthält mehr Einträge als die NAS-API vollständig anzeigen kann"
    );
    let mut existing: HashSet<_> = contents
        .entries
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    let mut report = Report {
        root: home.name,
        uploaded: 0,
        failures: vec![],
    };
    for source in files {
        let display = source.file_name().unwrap_or_default().to_string_lossy();
        let result = (|| -> Result<String> {
            let name = local_name(&source)?;
            let target = unique_name(name, &existing)?;
            nas.upload_named_progress(
                &report.root,
                FOLDER,
                &source,
                &target,
                Arc::new(Progress::default()),
            )?;
            Ok(target)
        })();
        match result {
            Ok(target) => {
                existing.insert(target);
                report.uploaded += 1;
            }
            Err(error) => report.failures.push(format!("{display}: {error:#}")),
        }
    }
    Ok(report)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collisions_preserve_extension_and_existing_names() {
        let existing = HashSet::from(["photo.jpg".into(), "photo (2).jpg".into()]);
        assert_eq!(
            unique_name("photo.jpg", &existing).unwrap(),
            "photo (3).jpg"
        );
        assert_eq!(unique_name("other.jpg", &existing).unwrap(), "other.jpg");
        assert_eq!(
            unique_name(".notes", &HashSet::from([".notes".into()])).unwrap(),
            ".notes (2)"
        );
    }
}
