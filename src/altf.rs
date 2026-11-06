//! Switching a box from Alt-F to ShareOS through Alt-F's own web interface,
//! the way docs/SWITCHING.md in the ShareOS repository describes it:
//! upload the ShareOS `.bin` (with its `.sha1`) on Alt-F's firmware page,
//! flash it, reboot. ShareOS then installs its system by itself (from a USB
//! stick or from GitHub) and starts with the first-setup wizard.
//!
//! Alt-F checks the file itself (signature, model, SHA-1) before it offers
//! flashing; this module additionally checks the download against the
//! release's SHA-256 file and refuses a model Alt-F does not report as
//! matching. Nothing here touches disks, U-Boot or the vendor's rescue system.
use anyhow::{bail, ensure, Context, Result};
use reqwest::blocking::Client as Http;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use url::Url;

/// Boxes ShareOS publishes a firmware for (asset name part, shown name).
pub const MODELS: [(&str, &str); 4] = [
    ("DNS-320L-rev-Ax", "D-Link DNS-320L (Rev. A)"),
    ("DNS-320-rev-Ax", "D-Link DNS-320 (Rev. A)"),
    ("DNS-320-rev-Bx", "D-Link DNS-320 (Rev. B)"),
    ("DNS-325-rev-Ax", "D-Link DNS-325 (Rev. A)"),
];
const RELEASES: &str = "https://api.github.com/repos/UniqueDroid/ShareOS/releases?per_page=15";

/// Does the web interface at `base` belong to Alt-F? (login page, no credentials sent)
pub fn detect(http: &Http, base: &Url) -> Result<bool> {
    let body = http
        .get(base.join("cgi-bin/login.cgi")?)
        .send()?
        .error_for_status()?
        .text()?;
    Ok(is_altf_login(&body))
}
fn is_altf_login(body: &str) -> bool {
    body.contains("Alt-F") && body.contains("login_proc.cgi") && body.contains("passwd")
}

#[derive(Clone)]
pub struct AltF {
    pub base: Url,
    http: Http,
    cookie: String,
}

/// What Alt-F said about the uploaded file.
#[derive(Debug, PartialEq)]
pub enum Checked {
    /// file accepted, "FlashIt" can be used now
    Ready(String),
    /// accepted, but Alt-F runs from the flash: a special reboot comes first
    NeedsSpecialReboot(String),
    /// refused (wrong model, broken file …): Alt-F's message
    Refused(String),
}

pub fn client(timeout: u64) -> Result<Http> {
    Ok(Http::builder()
        .timeout(Duration::from_secs(timeout))
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("ShareOS-Desktop/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

impl AltF {
    /// Alt-F's login: the password is sent as md5(password + salt), the salt
    /// comes with the login page; the answer sets the session cookie ALTFID.
    pub fn login(base: &Url, password: &str) -> Result<Self> {
        ensure!(
            base.scheme() == "http",
            "Alt-F wird über http:// angesprochen"
        );
        let http = client(900)?;
        let page = http
            .get(base.join("cgi-bin/login.cgi")?)
            .send()?
            .error_for_status()?
            .text()?;
        ensure!(is_altf_login(&page), "Das ist keine Alt-F-Anmeldeseite");
        ensure!(
            !page.contains("passwd_again"),
            "Alt-F ist noch nicht eingerichtet (kein Passwort gesetzt). Zuerst in Alt-F ein Passwort vergeben."
        );
        let salt = input_value(&page, "salt").context("Alt-F-Anmeldeseite ohne Salt")?;
        let hash = format!(
            "{:x}",
            md5::Md5::digest(format!("{password}{salt}").as_bytes())
        );
        let reply = http
            .post(base.join("cgi-bin/login_proc.cgi")?)
            .form(&[
                ("passwd", hash.as_str()),
                ("from_url", ""),
                ("salt", salt.as_str()),
            ])
            .send()?;
        let location = reply
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let cookie = reply
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(|v| {
                v.split(';')
                    .next()?
                    .trim()
                    .strip_prefix("ALTFID=")
                    .map(String::from)
            });
        match cookie {
            Some(id) if !location.contains("login.cgi") && !id.is_empty() => Ok(Self {
                base: base.clone(),
                http,
                cookie: format!("ALTFID={id}"),
            }),
            _ => bail!("Alt-F-Anmeldung fehlgeschlagen – Passwort prüfen"),
        }
    }

    /// Upload the firmware (and its SHA-1 file) to Alt-F's firmware page.
    pub fn upload(&self, bin_name: &str, bin: &[u8], sha1: &[u8]) -> Result<Checked> {
        let boundary = format!("----shareos{:016x}", rand_u64());
        let mut body = Vec::with_capacity(bin.len() + sha1.len() + 512);
        for (field, name, data) in [
            ("fw_bin", bin_name, bin),
            ("fw_sha1", "firmware.sha1", sha1),
        ] {
            body.extend_from_slice(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )
                .as_bytes(),
            );
            body.extend_from_slice(data);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        let page = self
            .http
            .post(self.base.join("cgi-bin/firmware_proc.cgi")?)
            .header(reqwest::header::COOKIE, &self.cookie)
            .header(
                reqwest::header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()?
            .error_for_status()?
            .text()?;
        Ok(checked(&page))
    }

    /// The buttons of the second firmware page: "SpecialReboot", "FlashIt", "Abort".
    /// Settings in the flash stay ("Don't erase flashed settings"): ShareOS
    /// takes users and shares over from there on its first start.
    pub fn flash(&self, action: &str) -> Result<String> {
        ensure!(
            matches!(action, "FlashIt" | "SpecialReboot" | "Abort"),
            "unbekannte Aktion"
        );
        let page = self
            .http
            .post(self.base.join("cgi-bin/firmware2_proc.cgi")?)
            .header(reqwest::header::COOKIE, &self.cookie)
            .form(&[("flash_defaults", "none"), ("flash", action)])
            .send()?
            .error_for_status()?
            .text()?;
        let text = plain(&page);
        if action == "FlashIt" {
            ensure!(
                !page.contains("Failed") && page.contains("reboot"),
                "Flashen fehlgeschlagen – die Box NICHT ausschalten. Alt-F meldet: {}",
                short(&text)
            );
        }
        Ok(text)
    }

    pub fn reboot(&self) -> Result<()> {
        // the box goes down while answering: a broken connection is fine
        let _ = self
            .http
            .post(self.base.join("cgi-bin/sys_utils_proc.cgi")?)
            .header(reqwest::header::COOKIE, &self.cookie)
            .form(&[("action", "Reboot")])
            .timeout(Duration::from_secs(20))
            .send();
        Ok(())
    }
}

fn checked(page: &str) -> Checked {
    let text = plain(page);
    if !page.contains("Everything looks OK") {
        return Checked::Refused(short(&text));
    }
    let flash_disabled = page
        .split('<')
        .any(|tag| tag.contains("value=\"FlashIt\"") && tag.contains("disabled"));
    if flash_disabled && page.contains("value=\"SpecialReboot\"") {
        Checked::NeedsSpecialReboot(short(&text))
    } else if flash_disabled {
        Checked::Refused(short(&text))
    } else {
        Checked::Ready(short(&text))
    }
}

/// the value of `<input … name=NAME … value="…">` (Alt-F writes attributes in both styles)
fn input_value(page: &str, name: &str) -> Option<String> {
    page.split('<').find_map(|tag| {
        let tag = tag.split('>').next()?;
        if !tag.starts_with("input")
            || !(tag.contains(&format!("name={name} "))
                || tag.contains(&format!("name=\"{name}\"")))
        {
            return None;
        }
        let v = tag.split("value=\"").nth(1)?.split('"').next()?;
        Some(v.to_string())
    })
}

/// HTML without tags, scripts and repeated whitespace
pub fn plain(page: &str) -> String {
    let mut out = String::new();
    let mut rest = page;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        out.push(' ');
        let after = &rest[i..];
        let lower = after.get(..8).unwrap_or("").to_ascii_lowercase();
        let end = if lower.starts_with("<script") || lower.starts_with("<style") {
            let close = if lower.starts_with("<script") {
                "</script>"
            } else {
                "</style>"
            };
            after
                .to_ascii_lowercase()
                .find(close)
                .map(|j| j + close.len())
        } else {
            after.find('>').map(|j| j + 1)
        };
        match end {
            Some(j) => rest = &after[j..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn short(text: &str) -> String {
    let t: String = text.chars().take(600).collect();
    if text.chars().count() > 600 {
        format!("{t} …")
    } else {
        t
    }
}
fn rand_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    h.finish()
}

/* ---------- the firmware from GitHub ---------- */

#[derive(Deserialize, Clone)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize, Clone)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// A downloaded and checked ShareOS firmware for one model.
pub struct Firmware {
    pub version: String,
    pub model: String,
    pub bin_name: String,
    pub bin: Vec<u8>,
    pub sha1: Vec<u8>,
    pub sha256: String,
}

/// Newest release (pre-releases included when `beta`) with a `.bin` for the model.
pub fn download(model: &str, beta: bool) -> Result<Firmware> {
    ensure!(MODELS.iter().any(|m| m.0 == model), "unbekanntes Modell");
    let http = client(600)?;
    let releases: Vec<Release> = http
        .get(RELEASES)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()?
        .error_for_status()
        .context("GitHub-Releases nicht erreichbar")?
        .json()?;
    let release = pick(&releases, model, beta)
        .context("Kein ShareOS-Release mit einer Firmware für dieses Modell gefunden")?;
    let version = release.tag_name.trim_start_matches('v').to_string();
    let bin_name = format!("ShareOS-{version}-{model}.bin");
    let asset = |name: &str| -> Result<&Asset> {
        release
            .assets
            .iter()
            .find(|a| a.name == name)
            .with_context(|| format!("{name} fehlt im Release"))
    };
    let get = |a: &Asset, max: u64| -> Result<Vec<u8>> {
        ensure!(a.size <= max, "{} ist unerwartet groß", a.name);
        let url = Url::parse(&a.browser_download_url)?;
        ensure!(
            url.scheme() == "https" && url.host_str() == Some("github.com"),
            "unerwartete Download-Adresse"
        );
        let mut r = Http::builder()
            .timeout(Duration::from_secs(600))
            .user_agent(concat!("ShareOS-Desktop/", env!("CARGO_PKG_VERSION")))
            .build()?
            .get(url)
            .send()?
            .error_for_status()?;
        let mut data = vec![];
        use std::io::Read;
        (&mut r).take(max + 1).read_to_end(&mut data)?;
        ensure!(data.len() as u64 <= max, "{} ist unerwartet groß", a.name);
        Ok(data)
    };
    let bin = get(asset(&bin_name)?, 16 << 20)?;
    let sha256_file = String::from_utf8(get(asset(&format!("{bin_name}.sha256"))?, 4096)?)?;
    let sha1 = get(asset(&format!("ShareOS-{version}-{model}.sha1"))?, 4096)?;
    let sha256 = verify(&bin, &sha256_file, &sha1)?;
    Ok(Firmware {
        version,
        model: model.into(),
        bin_name,
        bin,
        sha1,
        sha256,
    })
}

fn pick<'a>(releases: &'a [Release], model: &str, beta: bool) -> Option<&'a Release> {
    releases.iter().find(|r| {
        !r.draft
            && (beta || !r.prerelease)
            && r.assets
                .iter()
                .any(|a| a.name.ends_with(&format!("-{model}.bin")))
    })
}

/// The .bin against its .sha256 file and the .sha1 file Alt-F checks again.
fn verify(bin: &[u8], sha256_file: &str, sha1_file: &[u8]) -> Result<String> {
    let want = sha256_file
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let have = format!("{:x}", Sha256::digest(bin));
    ensure!(
        want.len() == 64 && want == have,
        "SHA-256 der Firmware stimmt nicht – Download beschädigt"
    );
    let sha1 = String::from_utf8_lossy(sha1_file);
    let want1 = sha1
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let have1 = format!("{:x}", sha1::Sha1::digest(bin));
    ensure!(
        want1 == have1,
        "SHA-1 der Firmware stimmt nicht – Download beschädigt"
    );
    ensure!(bin.len() > 1 << 20, "Firmware-Datei zu klein");
    Ok(have)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verify_rejects_bad_checksums() {
        let bin = vec![7u8; 2 << 20];
        let s256 = format!("{:x}  x.bin\n", Sha256::digest(&bin));
        let s1 = format!("{:x}  x.bin\n", sha1::Sha1::digest(&bin));
        assert!(verify(&bin, &s256, s1.as_bytes()).is_ok());
        assert!(verify(&bin, &format!("{}  x.bin", "0".repeat(64)), s1.as_bytes()).is_err());
        assert!(verify(&bin, &s256, b"0000").is_err());
        assert!(verify(&bin[..10], &s256, s1.as_bytes()).is_err());
    }
    #[test]
    fn picks_newest_release_with_the_model() {
        let r = |tag: &str, pre: bool, names: &[&str]| Release {
            tag_name: tag.into(),
            draft: false,
            prerelease: pre,
            assets: names
                .iter()
                .map(|n| Asset {
                    name: n.to_string(),
                    browser_download_url: String::new(),
                    size: 1,
                })
                .collect(),
        };
        let list = vec![
            r(
                "v0.3.0-beta.1",
                true,
                &["ShareOS-0.3.0-beta.1-DNS-325-rev-Ax.bin"],
            ),
            r(
                "v0.2.0-beta.20",
                true,
                &["ShareOS-0.2.0-beta.20-DNS-320L-rev-Ax.bin"],
            ),
            r("v0.1.0", false, &["ShareOS-0.1.0-DNS-320L-rev-Ax.bin"]),
        ];
        assert_eq!(
            pick(&list, "DNS-320L-rev-Ax", true).unwrap().tag_name,
            "v0.2.0-beta.20"
        );
        assert_eq!(
            pick(&list, "DNS-320L-rev-Ax", false).unwrap().tag_name,
            "v0.1.0"
        );
        assert!(pick(&list, "DNS-320-rev-Bx", true).is_none());
    }
    #[test]
    fn reads_alt_f_pages() {
        let login = r#"<form name="loginf" action="/cgi-bin/login_proc.cgi" method="post"><input type="password" name="passwd" id="passwd_id"><input type="hidden" name=from_url value=""><input type="hidden" id=salt_id name=salt value="1234-abcd"></form> Alt-F"#;
        assert!(is_altf_login(login));
        assert_eq!(input_value(login, "salt").as_deref(), Some("1234-abcd"));
        let ok = r#"<h4>Everything looks OK.<br></h4>Your box is a DNS-320L-Ax and this firmware is for the DNS-320L-Ax.<input type="submit"  name=flash value="FlashIt" onclick="x"><input type="submit" disabled name=flash value="TryIt">"#;
        assert!(matches!(checked(ok), Checked::Ready(_)));
        let special = r#"<h4>Everything looks OK.<br></h4><input type=submit name=flash value="SpecialReboot"><input type="submit" disabled name=flash value="FlashIt">"#;
        assert!(matches!(checked(special), Checked::NeedsSpecialReboot(_)));
        let wrong = r#"<script>alert(1)</script><p>Your box is a DNS-325-Ax and this firmware is for the DNS-320L-Ax</p>"#;
        match checked(wrong) {
            Checked::Refused(text) => {
                assert!(text.contains("DNS-325-Ax") && !text.contains("alert"))
            }
            _ => panic!("must be refused"),
        }
    }

    /// (method, path and header, body) of a request the test server got
    type Seen = (String, String, Vec<u8>);
    /// a tiny HTTP server: answers each request with `answer(method, path, headers, body)`
    fn server(
        count: usize,
        answer: impl Fn(&str, &str, &str, &[u8]) -> String + Send + 'static,
    ) -> (Url, std::thread::JoinHandle<Vec<Seen>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let handle = std::thread::spawn(move || {
            let mut seen = vec![];
            for _ in 0..count {
                let (mut sock, _) = listener.accept().unwrap();
                let mut head = vec![];
                while !head.ends_with(b"\r\n\r\n") {
                    let mut b = [0];
                    sock.read_exact(&mut b).unwrap();
                    head.push(b[0]);
                }
                let head = String::from_utf8(head).unwrap();
                let len: usize = head
                    .lines()
                    .find_map(|l| {
                        l.split_once(':')
                            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .map(|(_, v)| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                let mut body = vec![0; len];
                sock.read_exact(&mut body).unwrap();
                let mut first = head.lines().next().unwrap().split(' ');
                let (method, path) = (
                    first.next().unwrap().to_string(),
                    first.next().unwrap().to_string(),
                );
                let reply = answer(&method, &path, &head, &body);
                sock.write_all(reply.as_bytes()).unwrap();
                seen.push((method, format!("{path}\n{head}"), body));
            }
            seen
        });
        (base, handle)
    }
    fn html(body: &str) -> String {
        format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
    }
    #[test]
    fn logs_in_uploads_and_flashes_like_the_alt_f_forms() {
        let login_page = r#"<title>Alt-F Login</title><form name="loginf" action="/cgi-bin/login_proc.cgi" method="post"><input type="password" name="passwd" id="passwd_id"><input type="hidden" name=from_url value=""><input type="hidden" id=salt_id name=salt value="s4lt"></form>"#;
        let (base, server) = server(5, move |method, path, head, body| {
            match (method, path) {
            ("GET", "/cgi-bin/login.cgi") => html(login_page),
            ("POST", "/cgi-bin/login_proc.cgi") => {
                let form = String::from_utf8_lossy(body).to_string();
                let want = format!("{:x}", md5::Md5::digest(b"geheims4lt"));
                if form.contains(&format!("passwd={want}")) && form.contains("salt=s4lt") {
                    "HTTP/1.1 303\r\nSet-Cookie: ALTFID=abc-123; HttpOnly\r\nLocation: /cgi-bin/status.cgi\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()
                } else {
                    "HTTP/1.1 303\r\nLocation: /cgi-bin/login.cgi\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()
                }
            }
            ("POST", "/cgi-bin/firmware_proc.cgi") if head.to_ascii_lowercase().contains("cookie: altfid=abc-123") => {
                html(r#"<h4>Everything looks OK.<br></h4>Your box is a DNS-320L-Ax and this firmware is for the DNS-320L-Ax.<form><input type="submit" name=flash value="FlashIt"></form>"#)
            }
            ("POST", "/cgi-bin/firmware2_proc.cgi") if head.to_ascii_lowercase().contains("cookie: altfid=abc-123") => {
                html("<p>kernel: Erasing ... OK</p><form>You should reboot the box now for the new firmware to become active.</form>")
            }
            ("POST", "/cgi-bin/sys_utils_proc.cgi") => html("rebooting"),
            _ => "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        }
        });
        let alt = AltF::login(&base, "geheim").unwrap();
        let bin = vec![0x5au8; 3000];
        assert!(matches!(
            alt.upload("ShareOS-x-DNS-320L-rev-Ax.bin", &bin, b"0123  x\n")
                .unwrap(),
            Checked::Ready(_)
        ));
        assert!(alt.flash("FlashIt").unwrap().contains("reboot"));
        alt.reboot().unwrap();
        let seen = server.join().unwrap();
        // the upload is a multipart form with both files
        let upload = String::from_utf8_lossy(&seen[2].2);
        assert!(upload.contains("name=\"fw_bin\"; filename=\"ShareOS-x-DNS-320L-rev-Ax.bin\""));
        assert!(upload.contains("name=\"fw_sha1\""));
        assert_eq!(
            seen[2]
                .2
                .windows(3000)
                .filter(|w| w.iter().all(|b| *b == 0x5a))
                .count(),
            1
        );
        // flashing keeps the settings in the flash (users and shares for ShareOS)
        let flash = String::from_utf8_lossy(&seen[3].2);
        assert!(flash.contains("flash_defaults=none") && flash.contains("flash=FlashIt"));
        assert!(String::from_utf8_lossy(&seen[4].2).contains("action=Reboot"));
    }
    #[test]
    fn wrong_password_and_new_alt_f_are_refused() {
        let (base, first) = server(3, |method, path, _, _| {
            match (method, path) {
            ("GET", _) if path.starts_with("/cgi-bin/login.cgi") => html(r#"Alt-F <form action="/cgi-bin/login_proc.cgi"><input type="password" name="passwd"><input type="hidden" id=salt_id name=salt value="x"></form>"#),
            _ => "HTTP/1.1 303\r\nLocation: /cgi-bin/login.cgi\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        }
        });
        assert!(AltF::login(&base, "falsch").is_err());
        assert!(
            AltF::login(&Url::parse("https://box/").unwrap(), "x").is_err(),
            "Alt-F only over http"
        );
        let (base2, server2) = server(1, |_, _, _, _| {
            html(
                r#"Alt-F <form action="/cgi-bin/login_proc.cgi"><input type="password" name="passwd"><input type="password" name="passwd_again" id="passwd_again"></form>"#,
            )
        });
        assert!(AltF::login(&base2, "x")
            .err()
            .unwrap()
            .to_string()
            .contains("noch nicht eingerichtet"));
        drop(first);
        server2.join().unwrap();
    }
    #[test]
    fn a_failed_flash_is_an_error() {
        let (base, server) = server(1, |_, _, _, _| {
            html(r#"<p>kernel: Erasing ... <strong><span class="error">Failed</span></strong></p>"#)
        });
        let alt = AltF {
            base,
            http: client(10).unwrap(),
            cookie: "ALTFID=x".into(),
        };
        let e = alt.flash("FlashIt").unwrap_err().to_string();
        assert!(e.contains("NICHT ausschalten"));
        server.join().unwrap();
    }
}
