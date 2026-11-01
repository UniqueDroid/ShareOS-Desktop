use crate::{nas::Nas, transfer::Progress};
use anyhow::{ensure, Context, Result};
use std::sync::Arc;
#[derive(Clone)]
pub struct Clipboard {
    pub root: String,
    pub path: String,
    pub names: Vec<String>,
    origin: String,
    user: String,
}
impl Clipboard {
    pub fn new(nas: &Nas, root: String, path: String, names: Vec<String>) -> Self {
        Self {
            root,
            path,
            names,
            origin: nas.base.origin().ascii_serialization(),
            user: nas.user.clone(),
        }
    }
    pub fn check(&self, nas: &Nas) -> Result<()> {
        ensure!(
            self.origin == nas.base.origin().ascii_serialization() && self.user == nas.user,
            "Die Auswahl gehört zu einer anderen Anmeldung"
        );
        ensure!(
            !self.names.is_empty() && self.names.iter().all(|name| valid_name(name)),
            "Ungültige Dateiauswahl"
        );
        Ok(())
    }
    pub fn paste(&self, nas: &Nas, dest_root: &str, dest_path: &str) -> Result<()> {
        self.check(nas)?;
        if self.root == dest_root {
            return nas.copy(&self.root, &self.path, &self.names, dest_path);
        }
        // Plan the full tree before writes, so overlapping admin roots cannot
        // recursively copy newly created destination directories into themselves.
        let source = nas.list(&self.root, &self.path)?;
        let destination = nas.list(dest_root, dest_path)?;
        ensure!(
            !source.truncated && !destination.truncated,
            "Unvollständige Ordnerliste; Kopieren abgebrochen"
        );
        for name in &self.names {
            ensure!(
                !destination.entries.iter().any(|entry| &entry.name == name),
                "Ziel enthält bereits {name}"
            );
        }
        let mut directories = vec![];
        let mut files = vec![];
        let mut queue = vec![];
        for name in &self.names {
            let entry = source
                .entries
                .iter()
                .find(|entry| &entry.name == name)
                .context("Quelldatei ist nicht mehr vorhanden")?;
            match entry.kind.as_str(){"file"=>files.push((self.path.clone(),String::new(),name.clone())),"dir"=>{directories.push(name.clone());queue.push(name.clone());},_=>anyhow::bail!("Verknüpfungen und Spezialdateien können nur innerhalb desselben Bereichs kopiert werden")}
        }
        while let Some(relative) = queue.pop() {
            ensure!(
                relative.matches('/').count() < 128,
                "Ordnerstruktur ist zu tief"
            );
            let folder = nas.list(&self.root, &join(&self.path, &relative))?;
            ensure!(
                !folder.truncated,
                "Unvollständige Ordnerliste; Kopieren abgebrochen"
            );
            for entry in folder.entries {
                ensure!(valid_name(&entry.name), "Ungültiger Quellname");
                match entry.kind.as_str(){
                    "file"=>files.push((join(&self.path,&relative),relative.clone(),entry.name)),
                    "dir"=>{let path=join(&relative,&entry.name);directories.push(path.clone());queue.push(path);},
                    _=>anyhow::bail!("Verknüpfungen und Spezialdateien können nur innerhalb desselben Bereichs kopiert werden"),
                }
            }
        }
        directories.sort_by_key(|path| path.matches('/').count());
        for relative in directories {
            let (parent, name) = relative.rsplit_once('/').unwrap_or(("", &relative));
            nas.mkdir(dest_root, &join(dest_path, parent), name)?;
        }
        let staging = tempfile::tempdir()?;
        let local = staging.path().join("transfer.data");
        for (source_path, relative, name) in files {
            nas.download(&self.root, &source_path, &name, &local)?;
            nas.upload_named_progress(
                dest_root,
                &join(dest_path, &relative),
                &local,
                &name,
                Arc::new(Progress::default()),
            )?;
            std::fs::remove_file(&local)?;
        }
        Ok(())
    }
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.chars().any(char::is_control)
}
fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.into()
    } else if name.is_empty() {
        parent.into()
    } else {
        format!("{parent}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipboard_cannot_be_reused_by_another_user_or_box() {
        let nas = Nas::test_session(false);
        let clip = Clipboard::new(&nas, "home".into(), "".into(), vec!["file.txt".into()]);
        let mut other = nas.clone();
        other.user = "another".into();
        assert!(clip.check(&other).is_err());
        other.user = nas.user.clone();
        other.base = crate::nas::base_url("https://other.local").unwrap();
        assert!(clip.check(&other).is_err());
        assert!(clip.check(&nas).is_ok());
    }
}
