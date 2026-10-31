use crate::nas::base_url;
use anyhow::{ensure, Result};
use std::process::Command;

pub fn connect(address: &str, share: &str, drive: &str) -> Result<String> {
    let url = base_url(address)?;
    let host = url.host_str().unwrap();
    ensure!(
        !share.is_empty()
            && !share.starts_with('-')
            && !share.contains(['/', '\\', ':'])
            && !share.chars().any(char::is_control),
        "Ungültiger Freigabename"
    );
    #[cfg(target_os = "windows")]
    {
        ensure!(
            drive.len() == 2 && drive.as_bytes()[0].is_ascii_alphabetic() && drive.ends_with(':'),
            "Laufwerksbuchstabe wie S: eingeben"
        );
        let status = Command::new("net.exe")
            .args([
                "use",
                drive,
                &format!("\\\\{host}\\{share}"),
                "/persistent:no",
            ])
            .status()?;
        ensure!(status.success(), "Netzlaufwerk konnte nicht verbunden werden. Zugangsdaten zuerst in der Windows-Anmeldeinformationsverwaltung hinterlegen.");
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = drive;
        let mut smb = url::Url::parse("smb://localhost/")?;
        smb.set_host(Some(host))?;
        smb.path_segments_mut().unwrap().push(share);
        #[cfg(target_os = "macos")]
        let status = Command::new("open").arg(smb.as_str()).status()?;
        #[cfg(not(target_os = "macos"))]
        let status = Command::new("gio").args(["mount", smb.as_str()]).status()?;
        ensure!(
            status.success(),
            "SMB-Verbindung fehlgeschlagen. System-Anmeldung und SMB-Unterstützung prüfen."
        );
    }
    Ok("Verbindung an das Betriebssystem übergeben. SMB verwendet dessen eigene Anmeldung.".into())
}
