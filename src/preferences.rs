use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Default, Serialize, Deserialize)]
pub struct Preferences {
    pub address: Option<String>,
    pub certificate: Option<crate::tls::Certificate>,
    /// folder of the settings backups, when not the default one
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backups: Option<String>,
}
impl Preferences {
    pub fn location() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        let base = std::env::var_os("APPDATA").map(PathBuf::from);
        #[cfg(target_os = "macos")]
        let base = std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join("Library/Application Support"));
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
        base.map(|base| base.join("shareos-desktop/connection.json"))
    }
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let value: Self = serde_json::from_reader(fs::File::open(path)?)?;
        if let Some(address) = &value.address {
            crate::nas::base_url(address)?;
        }
        if let Some(cert) = &value.certificate {
            let address = value
                .address
                .as_deref()
                .context("Gespeicherte Zertifikatsadresse fehlt")?;
            anyhow::ensure!(
                cert.origin
                    == crate::nas::base_url(address)?
                        .origin()
                        .ascii_serialization()
                    && !cert.der.is_empty(),
                "Ungültiges gespeichertes Zertifikatsvertrauen"
            );
        }
        Ok(value)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path.parent().context("Einstellungsordner fehlt")?;
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
        // The file contains only the box address and public certificate, never credentials.
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        serde_json::to_writer_pretty(&mut file, self)?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saves_only_explicit_address_and_matching_certificate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs/connection.json");
        let value = Preferences {
            address: Some("https://box.local".into()),
            certificate: Some(crate::tls::Certificate {
                origin: "https://box.local".into(),
                der: vec![1, 2, 3],
            }),
            backups: None,
        };
        value.save(&path).unwrap();
        let loaded = Preferences::load(&path).unwrap();
        assert_eq!(loaded.certificate.unwrap().der, vec![1, 2, 3]);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(!contents.contains("password") && !contents.contains("session"));
        Preferences::default().save(&path).unwrap();
        assert!(Preferences::load(&path).unwrap().certificate.is_none());
    }
}
