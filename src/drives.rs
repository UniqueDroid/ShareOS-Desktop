use crate::nas::base_url;
use anyhow::{ensure, Result};
use std::process::Command;

pub fn connect(address: &str, share: &str, drive: &str, user: &str) -> Result<String> {
    let url = base_url(address)?;
    let host = url.host_str().unwrap();
    #[cfg(target_os = "windows")]
    let _ = user;
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
        if !user.is_empty() {
            smb.set_username(user)
                .map_err(|_| anyhow::anyhow!("Ungültiger SMB-Benutzer"))?;
        }
        smb.path_segments_mut().unwrap().push(share);
        #[cfg(target_os = "macos")]
        let status = Command::new("open").arg(smb.as_str()).status()?;
        #[cfg(not(target_os = "macos"))]
        let status = match Command::new("xdg-open").arg(smb.as_str()).output() {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Command::new("gio").args(["open", smb.as_str()]).output()?
            }
            Err(error) => return Err(error.into()),
        };
        #[cfg(target_os = "macos")]
        ensure!(
            status.success(),
            "SMB-Verbindung fehlgeschlagen. Anmeldung im Finder prüfen."
        );
        #[cfg(not(target_os = "macos"))]
        ensure!(status.status.success(),"SMB-Dateimanager konnte nicht geöffnet werden: {}. Unter KDE kio-extras, sonst GVfs-SMB und den Standard-Dateimanager prüfen.",String::from_utf8_lossy(&status.stderr).trim());
    }
    #[cfg(target_os = "windows")]
    return Ok("Netzlaufwerk verbunden.".into());
    #[cfg(not(target_os = "windows"))]
    Ok("Freigabe im System-Dateimanager geöffnet. Dort mit deinem SMB-Konto anmelden.".into())
}
