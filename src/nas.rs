use anyhow::{anyhow, bail, ensure, Context, Result};
use reqwest::blocking::Client as Http;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs::File, path::Path, time::Duration};
use url::Url;

const ZERO: &str = "00000000000000000000000000000000";
#[derive(Clone)]
pub struct Nas {
    pub base: Url,
    http: Http,
    sid: String,
    pub user: String,
    pub admin: bool,
    pub shares: Vec<Share>,
    pub trusted: Option<crate::tls::Certificate>,
}
#[derive(Clone, Deserialize, Debug)]
pub struct Share {
    pub name: String,
    pub access: String,
    #[serde(default)]
    pub comment: String,
}
fn user_shares(me: &Value) -> Vec<Share> {
    let mut shares: Vec<Share> = serde_json::from_value(me["shares"].clone()).unwrap_or_default();
    if let Some(name) = me["home"]["smb"].as_str().filter(|name| !name.is_empty()) {
        if !shares.iter().any(|share| share.name == name) {
            shares.insert(
                0,
                Share {
                    name: name.into(),
                    access: "rw".into(),
                    comment: "Eigene Dateien".into(),
                },
            );
        }
    }
    shares
}
#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct Root {
    pub name: String,
    #[serde(default)]
    pub home: bool,
    #[serde(default)]
    pub ro: bool,
}
#[derive(Clone, Deserialize, Debug)]
pub struct Entry {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub size: u64,
    #[serde(default)]
    pub mtime: Option<i64>,
}
#[derive(Deserialize)]
pub struct Listing {
    pub entries: Vec<Entry>,
    #[serde(default)]
    pub truncated: bool,
}
pub fn base_url(input: &str) -> Result<Url> {
    let input = input.trim();
    let url = Url::parse(&if input.contains("://") {
        input.into()
    } else {
        format!("https://{input}")
    })?;
    ensure!(
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
        "HTTP(S)-Box-Adresse erforderlich"
    );
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none(),
        "Nur Host und optional Port eingeben"
    );
    Ok(url)
}
impl Nas {
    #[cfg(test)]
    pub(crate) fn test_session(admin: bool) -> Self {
        Self {
            base: base_url("https://box.local").unwrap(),
            http: Http::new(),
            sid: "test".into(),
            user: "test".into(),
            admin,
            shares: vec![],
            trusted: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn test_at(base: url::Url, admin: bool) -> Self {
        let mut nas = Self::test_session(admin);
        nas.base = base;
        nas
    }
    /// Not logged in: only the public calls of the box (login page,
    /// claiming a new box).
    pub fn anonymous(address: &str, trusted: Option<&crate::tls::Certificate>) -> Result<Self> {
        let base = base_url(address)?;
        Ok(Self {
            base: base.clone(),
            http: crate::tls::configure(
                Http::builder()
                    .timeout(Duration::from_secs(600))
                    .connect_timeout(Duration::from_secs(5))
                    .redirect(reqwest::redirect::Policy::none()),
                &base,
                trusted,
            )?
            .build()?,
            sid: ZERO.into(),
            user: String::new(),
            admin: false,
            shares: vec![],
            trusted: trusted.cloned(),
        })
    }

    /// A new box (firmware 0.3.0-beta.2 and newer) keeps root locked until
    /// it is claimed: USB button on the box, then the first password.
    /// None: firmware without claiming, or the box is claimed.
    pub fn claim_state(&self) -> Result<Option<Value>> {
        match self.rpc("shareos.setup", "claim_state", json!({})) {
            Ok(st) if st["unclaimed"].as_bool() == Some(true) => Ok(Some(st)),
            Ok(_) => Ok(None),
            Err(e) if crate::tls::is_certificate_error(&e) => Err(e),
            Err(_) => Ok(None),
        }
    }

    /// The first password of a new box (within 2 minutes after the USB button).
    pub fn claim(&self, password: &str) -> Result<()> {
        self.rpc("shareos.setup", "claim", json!({ "password": password }))
            .map(|_| ())
            .map_err(|e| anyhow!(claim_error(&e.to_string())))
    }

    pub fn connect(
        address: &str,
        user: String,
        password: &str,
        trusted: Option<&crate::tls::Certificate>,
    ) -> Result<Self> {
        let mut nas = Self::anonymous(address, trusted)?;
        nas.user = user;
        let result = nas.rpc(
            "session",
            "login",
            json!({"username":nas.user,"password":password,"timeout":3600}),
        ).context("Anmeldung fehlgeschlagen. Bei UnknownIssuer oder Zertifikatsfehler: Zertifikat prüfen, Fingerabdruck mit der Box vergleichen und ausdrücklich vertrauen.")?;
        nas.sid = result["ubus_rpc_session"]
            .as_str()
            .context("Keine Sitzung erhalten")?
            .into();
        let bound = nas.rpc(
            "shareos.me",
            "bind",
            json!({"username":nas.user,"password":password}),
        );
        match bound {
            Ok(me) => {
                nas.admin = me["role"].as_str() == Some("admin");
                nas.shares = user_shares(&me);
            }
            Err(error) => {
                let _ = nas.logout();
                return Err(error.context("Benutzerzuordnung fehlgeschlagen"));
            }
        }
        Ok(nas)
    }
    pub fn rpc(&self, object: &str, method: &str, args: Value) -> Result<Value> {
        let response: Value = self
            .http
            .post(self.base.join("ubus")?)
            .json(&json!({
                "jsonrpc":"2.0","id":1,"method":"call","params":[self.sid,object,method,args]
            }))
            .send()?
            .error_for_status()?
            .json()?;
        decode(response)
    }
    pub fn logout(&self) -> Result<()> {
        self.rpc("session", "destroy", json!({"ubus_rpc_session":self.sid}))?;
        Ok(())
    }
    pub fn roots(&self) -> Result<Vec<Root>> {
        Ok(serde_json::from_value(
            self.rpc("shareos.files", "roots", json!({}))?["roots"].clone(),
        )?)
    }
    pub fn list(&self, root: &str, path: &str) -> Result<Listing> {
        Ok(serde_json::from_value(self.rpc(
            "shareos.files",
            "list",
            json!({"root":root,"path":path,"hidden":true}),
        )?)?)
    }
    fn ticket(&self, root: &str, path: &str, name: &str, mode: &str) -> Result<Url> {
        let data = self.rpc(
            "shareos.files",
            "ticket",
            json!({"root":root,"path":path,"name":name,"mode":mode,"overwrite":false}),
        )?;
        let url = self
            .base
            .join(data["url"].as_str().context("Ticket fehlt")?)?;
        ensure!(
            url.origin() == self.base.origin() && url.path() == "/cgi-bin/files",
            "Ungültige Ticket-Adresse"
        );
        Ok(url)
    }
    // create_new prevents an existing local file from being overwritten.
    pub fn download(&self, root: &str, path: &str, name: &str, target: &Path) -> Result<()> {
        self.download_progress(
            root,
            path,
            name,
            target,
            &crate::transfer::Progress::default(),
        )
    }
    pub fn download_progress(
        &self,
        root: &str,
        path: &str,
        name: &str,
        target: &Path,
        progress: &crate::transfer::Progress,
    ) -> Result<()> {
        use std::io::{Read, Write};
        let url = self.ticket(root, path, name, "get")?;
        let mut response = self.http.get(url).send()?.error_for_status()?;
        progress.total(response.content_length().unwrap_or(0));
        let mut file = private_file(target)?;
        let result = (|| -> Result<()> {
            let mut buffer = [0u8; 65536];
            loop {
                let n = response.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                file.write_all(&buffer[..n])?;
                progress.add(n);
            }
            file.sync_all()?;
            Ok(())
        })();
        if let Err(error) = result {
            drop(file);
            let _ = std::fs::remove_file(target);
            return Err(error);
        }
        Ok(())
    }
    pub fn upload_progress(
        &self,
        root: &str,
        path: &str,
        source: &Path,
        progress: std::sync::Arc<crate::transfer::Progress>,
    ) -> Result<()> {
        let name = source
            .file_name()
            .and_then(|s| s.to_str())
            .context("Dateiname ist kein UTF-8")?;
        self.upload_named_progress(root, path, source, name, progress)
    }
    pub fn upload_named_progress(
        &self,
        root: &str,
        path: &str,
        source: &Path,
        name: &str,
        progress: std::sync::Arc<crate::transfer::Progress>,
    ) -> Result<()> {
        let file = File::open(source)?;
        let total = file.metadata()?.len();
        progress.total(total);
        let url = self.ticket(root, path, name, "put")?;
        let body = reqwest::blocking::Body::sized(
            crate::transfer::Reader {
                inner: file,
                progress,
            },
            total,
        );
        let data: Value = self
            .http
            .post(url)
            .body(body)
            .send()?
            .error_for_status()?
            .json()?;
        ensure!(data["ok"] == true, "Übertragung fehlgeschlagen");
        Ok(())
    }
    pub fn mkdir(&self, root: &str, path: &str, name: &str) -> Result<()> {
        ensure!(
            crate::file_ops::valid_name(name),
            "Gültigen Ordnernamen angeben"
        );
        self.rpc(
            "shareos.files",
            "mkdir",
            json!({"root":root,"path":path,"name":name}),
        )?;
        Ok(())
    }
    pub fn rename(&self, root: &str, path: &str, name: &str, to: &str) -> Result<()> {
        ensure!(
            crate::file_ops::valid_name(name) && crate::file_ops::valid_name(to),
            "Ungültiger Dateiname"
        );
        self.rpc(
            "shareos.files",
            "rename",
            json!({"root":root,"path":path,"name":name,"to":to}),
        )?;
        Ok(())
    }
    pub fn delete(&self, root: &str, path: &str, names: &[String]) -> Result<()> {
        ensure!(
            !names.is_empty() && names.iter().all(|name| crate::file_ops::valid_name(name)),
            "Ungültige Auswahl"
        );
        self.rpc(
            "shareos.files",
            "delete",
            json!({"root":root,"path":path,"names":names,"permanent":false}),
        )?;
        Ok(())
    }
    pub fn copy(&self, root: &str, path: &str, names: &[String], dest: &str) -> Result<()> {
        ensure!(
            !names.is_empty() && names.iter().all(|name| crate::file_ops::valid_name(name)),
            "Ungültige Auswahl"
        );
        self.rpc(
            "shareos.files",
            "move",
            json!({"root":root,"path":path,"names":names,"dest":dest,"copy":true}),
        )?;
        Ok(())
    }
    pub fn create_file(&self, root: &str, path: &str, name: &str) -> Result<()> {
        ensure!(crate::file_ops::valid_name(name), "Ungültiger Dateiname");
        let url = self.ticket(root, path, name, "put")?;
        let data: Value = self
            .http
            .post(url)
            .header(reqwest::header::CONTENT_LENGTH, 0)
            .body(Vec::<u8>::new())
            .send()?
            .error_for_status()?
            .json()?;
        ensure!(data["ok"] == true, "Datei konnte nicht angelegt werden");
        Ok(())
    }
    pub fn refresh_shares(&self) -> Result<Vec<Share>> {
        Ok(user_shares(&self.rpc("shareos.me", "get", json!({}))?))
    }
    pub fn terminal_token(&self, url: &Url) -> Result<String> {
        ensure!(
            url.host_str() == self.base.host_str() && url.scheme() == self.base.scheme(),
            "Ungültige Terminal-Adresse"
        );
        let value: Value = self
            .http
            .get(url.join("token")?)
            .send()
            .map_err(reqwest::Error::without_url)?
            .error_for_status()
            .map_err(reqwest::Error::without_url)?
            .json()?;
        Ok(value["token"].as_str().unwrap_or("").into())
    }
    pub fn terminal(&self, dark: bool) -> Result<Url> {
        ensure!(self.admin, "Administratorrechte erforderlich");
        let p = crate::theme::palette(dark);
        let hex = |c: eframe::egui::Color32| format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
        let data=self.rpc("shareos.system","terminal",json!({"tls":self.base.scheme()=="https","theme":{
            "background":hex(p.bg),"foreground":hex(p.text),"cursor":hex(p.accent),"selectionBackground":hex(p.dim)
        }}))?;
        terminal_url(&self.base, &data)
    }
    /// certificate / ca: with their keys – the box allows it only encrypted
    pub fn settings(&self, password: &str, certificate: bool, ca: bool) -> Result<Value> {
        ensure!(self.admin, "Administratorrechte erforderlich");
        self.rpc(
            "shareos.backup",
            "export",
            export_args(password, certificate, ca),
        )
    }
    pub fn restore_settings(&self, content: &str, password: &str, apply: bool) -> Result<Value> {
        ensure!(self.admin, "Administratorrechte erforderlich");
        self.rpc(
            "shareos.backup",
            "restore",
            json!({"content":content,"password":password,"apply":apply}),
        )
    }
}
/// rpcd refuses arguments a method does not declare (code 2): firmware before
/// 0.2.0-beta.21 knows only the password, so the options go along only when set.
fn export_args(password: &str, certificate: bool, ca: bool) -> Value {
    let mut args = json!({ "password": password });
    if certificate {
        args["certificate"] = json!(true);
    }
    if ca {
        args["ca"] = json!(true);
    }
    args
}
/// German text for the answers of shareos.setup claim.
fn claim_error(e: &str) -> String {
    if e.contains("press the USB button") {
        "Zuerst die USB-Taste vorne an der Box kurz drücken – danach bleiben 2 Minuten für das Passwort.".into()
    } else if e.contains("too short") {
        "Das Passwort braucht mindestens 8 Zeichen.".into()
    } else if e.contains("choose your own") {
        "Bitte ein eigenes Passwort wählen, nicht „shareos“.".into()
    } else if e.contains("already claimed") {
        "Die Box hat schon ein Passwort – bitte damit anmelden.".into()
    } else {
        e.to_string()
    }
}

fn terminal_url(base: &Url, data: &Value) -> Result<Url> {
    let port = data["port"]
        .as_u64()
        .filter(|n| (1..=65535).contains(n))
        .context("Ungültiger Terminal-Port")? as u16;
    let path = data["path"]
        .as_str()
        .filter(|s| s.len() == 32 && s.bytes().all(|c| c.is_ascii_hexdigit()))
        .context("Ungültige Terminal-Sitzung")?;
    let mut url = base.clone();
    url.set_port(Some(port))
        .map_err(|_| anyhow!("Ungültiger Terminal-Port"))?;
    url.set_path(&format!("/{path}/"));
    Ok(url)
}
fn decode(response: Value) -> Result<Value> {
    if let Some(error) = response.get("error") {
        bail!(
            "RPC: {}",
            error["message"].as_str().unwrap_or("Sitzung ungültig")
        );
    }
    let result = response["result"]
        .as_array()
        .context("Ungültige RPC-Antwort")?;
    ensure!(
        result.first().and_then(Value::as_u64) == Some(0),
        "RPC verweigert (Code {}). Erneut anmelden, wenn die Sitzung abgelaufen ist.",
        result.first().unwrap_or(&Value::Null)
    );
    let data = result.get(1).cloned().unwrap_or(json!({}));
    if let Some(error) = data.get("error") {
        return Err(anyhow!("NAS: {error}"));
    }
    Ok(data)
}
pub fn private_file(path: &Path) -> Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}
/// A box found on the network: ShareOS, or Alt-F (which can be switched to ShareOS).
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub address: String,
    pub altf: bool,
}
// Public identification only: discovery never sends credentials or trusts a
// discovered certificate for subsequent login.
pub fn discover(prefix: &str, configured: &str) -> Result<Vec<Found>> {
    let mut addresses = discovery_addresses(prefix, configured)?;
    if prefix.trim().is_empty() {
        for host in local_search_hosts()? {
            for scheme in ["https", "http"] {
                let address = format!("{scheme}://{host}/");
                if !addresses.contains(&address) {
                    addresses.push(address);
                }
            }
        }
    }
    let http = Http::builder()
        .timeout(Duration::from_secs(2))
        .connect_timeout(Duration::from_secs(1))
        .danger_accept_invalid_certs(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut found = Vec::new();
    for batch in addresses.chunks(32) {
        std::thread::scope(|scope| {
            let jobs: Vec<_> = batch
                .iter()
                .map(|address| {
                    let http = &http;
                    scope.spawn(move || {
                        if probe(http, address).unwrap_or(false) {
                            return Some(Found {
                                address: address.clone(),
                                altf: false,
                            });
                        }
                        // Alt-F only answers on plain HTTP
                        let base = base_url(address).ok().filter(|b| b.scheme() == "http")?;
                        crate::altf::detect(http, &base)
                            .unwrap_or(false)
                            .then(|| Found {
                                address: address.clone(),
                                altf: true,
                            })
                    })
                })
                .collect();
            for job in jobs {
                if let Ok(Some(box_found)) = job.join() {
                    if !found.contains(&box_found) {
                        found.push(box_found);
                    }
                }
            }
        });
    }
    Ok(found)
}
fn network_hosts(ip: std::net::Ipv4Addr, mask: std::net::Ipv4Addr) -> Vec<std::net::Ipv4Addr> {
    let ip = u32::from(ip);
    let mut mask = u32::from(mask);
    // Avoid scanning tens of thousands of hosts on corporate /16 networks.
    if mask.count_ones() < 22 {
        mask = 0xffff_ff00;
    }
    let network = ip & mask;
    let broadcast = network | !mask;
    if broadcast.saturating_sub(network) <= 1 {
        return vec![];
    }
    ((network + 1)..broadcast)
        .filter(|host| *host != ip)
        .map(std::net::Ipv4Addr::from)
        .collect()
}
fn local_search_hosts() -> Result<Vec<std::net::Ipv4Addr>> {
    use std::net::{IpAddr, UdpSocket};
    // UDP connect selects a local route without sending a packet.
    let preferred = UdpSocket::bind("0.0.0.0:0").ok().and_then(|socket| {
        socket.connect("192.0.2.1:9").ok()?;
        Some(socket.local_addr().ok()?.ip())
    });
    let interfaces =
        if_addrs::get_if_addrs().context("Lokale Netzwerkadressen konnten nicht erkannt werden")?;
    let mut networks = vec![];
    for interface in interfaces {
        let if_addrs::IfAddr::V4(address) = interface.addr else {
            continue;
        };
        if address.ip.is_loopback() || address.ip.is_unspecified() || address.ip.is_link_local() {
            continue;
        }
        let is_default = preferred == Some(IpAddr::V4(address.ip));
        if !is_default
            && (interface.name.starts_with("docker")
                || interface.name.starts_with("veth")
                || interface.name.starts_with("br-"))
        {
            continue;
        }
        networks.push((is_default, address.ip, address.netmask));
    }
    // Prefer the default-route interface; fall back to detected LAN interfaces.
    if networks.iter().any(|network| network.0) {
        networks.retain(|network| network.0);
    }
    let mut hosts = vec![];
    for (_, ip, mask) in networks {
        for host in network_hosts(ip, mask) {
            if !hosts.contains(&host) {
                hosts.push(host);
            }
        }
    }
    Ok(hosts)
}
fn discovery_addresses(prefix: &str, configured: &str) -> Result<Vec<String>> {
    let mut addresses = Vec::new();
    if let Ok(base) = base_url(configured) {
        addresses.push(base.to_string());
    }
    for host in ["shareos.local", "192.168.0.32"] {
        for scheme in ["https", "http"] {
            addresses.push(format!("{scheme}://{host}/"));
        }
    }
    if !prefix.trim().is_empty() {
        let parts: Vec<_> = prefix.trim().split('.').collect();
        ensure!(
            parts.len() == 3 && parts.iter().all(|x| x.parse::<u8>().is_ok()),
            "Subnetz als 192.168.0 eingeben"
        );
        for i in 1..255 {
            for scheme in ["https", "http"] {
                addresses.push(format!("{scheme}://{}.{i}/", prefix.trim()));
            }
        }
    }
    let mut unique = std::collections::HashSet::new();
    addresses.retain(|address| unique.insert(address.clone()));
    Ok(addresses)
}
pub(crate) fn probe(http: &Http, address: &str) -> Result<bool> {
    use std::io::Read;
    let response = http
        .get(base_url(address)?.join("app.js")?)
        .send()?
        .error_for_status()?;
    // Redirects and generic login pages are not evidence of ShareOS.
    if !response.status().is_success() {
        return Ok(false);
    }
    let mut body = String::new();
    response.take(2 * 1024 * 1024).read_to_string(&mut body)?;
    Ok(body.contains("shareos.files") && body.contains("shareos.me"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_discovery_uses_local_subnet_and_bounds_large_networks() {
        use std::net::Ipv4Addr;
        let ip = Ipv4Addr::new(10, 242, 1, 20);
        let hosts = network_hosts(ip, Ipv4Addr::new(255, 255, 255, 0));
        assert!(hosts.contains(&Ipv4Addr::new(10, 242, 1, 32)));
        assert!(!hosts.contains(&ip));
        assert_eq!(hosts.len(), 253);
        let large = network_hosts(ip, Ipv4Addr::new(255, 255, 0, 0));
        assert_eq!(large.len(), 253);
        assert!(!large.contains(&Ipv4Addr::new(10, 242, 2, 1)));
    }
    #[test]
    fn discovery_checks_https_subnets_and_configured_dns_name() {
        let addresses = discovery_addresses("10.242.1", "https://shareos.skynet.lan").unwrap();
        for expected in [
            "https://shareos.skynet.lan/",
            "https://10.242.1.32/",
            "http://10.242.1.32/",
            "https://192.168.0.32/",
        ] {
            assert!(addresses.iter().any(|address| address == expected));
        }
        assert!(discovery_addresses("10.242.999", "").is_err());
    }
    #[test]
    fn rejects_unsafe_addresses() {
        for url in [
            "file:///etc/passwd",
            "https://user:pass@box",
            "https://box/ubus",
            "https://box/?x=1",
        ] {
            assert!(base_url(url).is_err());
        }
        assert_eq!(base_url("192.168.0.32").unwrap().scheme(), "https");
    }
    #[test]
    fn rpc_fails_closed() {
        for reply in [
            json!({"result":[6]}),
            json!({"result":[0,{"error":"denied"}]}),
            json!({"error":{"message":"expired"}}),
            json!({}),
        ] {
            assert!(decode(reply).is_err());
        }
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    fn mock(replies: Vec<Value>) -> (String, thread::JoinHandle<Vec<Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let mut calls = vec![];
            for reply in replies {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut header = vec![];
                while !header.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    socket.read_exact(&mut byte).unwrap();
                    header.push(byte[0]);
                }
                let header = String::from_utf8(header).unwrap();
                let length: usize = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                let mut body = vec![0; length];
                socket.read_exact(&mut body).unwrap();
                calls.push(serde_json::from_slice(&body).unwrap());
                let body = reply.to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
            calls
        });
        (address, handle)
    }
    fn reply(value: Value) -> Value {
        json!({"jsonrpc":"2.0","id":1,"result":[0,value]})
    }
    #[test]
    fn new_box_is_claimed_without_login() {
        let (address, server) = mock(vec![
            reply(json!({"unclaimed":true,"window":false,"seconds":120})),
            reply(json!({"error":"press the USB button first"})),
            reply(json!({"ok":true})),
            // older firmware: the method is not there (access denied)
            json!({"jsonrpc":"2.0","id":1,"error":{"code":-32002,"message":"Access denied"}}),
            reply(json!({"unclaimed":false,"window":false,"seconds":120})),
        ]);
        let nas = Nas::anonymous(&address, None).unwrap();
        assert!(nas.claim_state().unwrap().is_some());
        let e = nas.claim("geheim123").unwrap_err().to_string();
        assert!(e.contains("USB-Taste"), "{e}");
        nas.claim("geheim123").unwrap();
        assert!(nas.claim_state().unwrap().is_none());
        assert!(nas.claim_state().unwrap().is_none());
        let calls = server.join().unwrap();
        assert!(calls.iter().all(|c| c["params"][0] == ZERO));
        assert_eq!(calls[1]["params"][1], "shareos.setup");
        assert_eq!(calls[1]["params"][2], "claim");
        assert_eq!(calls[1]["params"][3]["password"], "geheim123");
    }
    #[test]
    fn user_session_is_bound_and_cannot_export_admin_settings() {
        let (address, server) = mock(vec![
            reply(json!({"ubus_rpc_session":"abc"})),
            reply(json!({"role":"user"})),
            reply(
                json!({"roots":[{"name":"alice","home":true,"ro":false},{"name":"Team","ro":true}]}),
            ),
        ]);
        let nas = Nas::connect(&address, "alice".into(), "test-password", None).unwrap();
        assert!(!nas.admin);
        assert!(nas.settings("", false, false).is_err());
        assert!(nas.terminal(false).is_err());
        let roots = nas.roots().unwrap();
        assert!(roots[0].home && roots[1].ro);
        let calls = server.join().unwrap();
        assert_eq!(calls[0]["params"][0], ZERO);
        assert_eq!(calls[1]["params"][0], "abc");
        assert_eq!(calls[1]["params"][1], "shareos.me");
        assert_eq!(calls[1]["params"][2], "bind");
    }
    #[test]
    fn admin_terminal_uses_existing_rpc_and_keeps_box_host_and_tls() {
        let token = "0123456789abcdef0123456789abcdef";
        let (address, server) = mock(vec![
            reply(json!({"ubus_rpc_session":"abc"})),
            reply(json!({"role":"admin"})),
            reply(json!({"port":7681,"path":token})),
        ]);
        let nas = Nas::connect(&address, "root".into(), "test-password", None).unwrap();
        let url = nas.terminal(true).unwrap();
        assert_eq!(url.host_str(), nas.base.host_str());
        assert_eq!(url.scheme(), nas.base.scheme());
        assert_eq!(url.port(), Some(7681));
        assert_eq!(url.path(), format!("/{token}/"));
        let calls = server.join().unwrap();
        assert_eq!(calls[2]["params"][1], "shareos.system");
        assert_eq!(calls[2]["params"][2], "terminal");
        assert_eq!(calls[2]["params"][3]["theme"]["background"], "#06090d");
        assert_eq!(calls[2]["params"][3]["tls"], false);
    }
    #[test]
    fn terminal_rejects_invalid_port_or_session_path() {
        let base = base_url("https://box.local").unwrap();
        for data in [
            json!({"port":0,"path":"0123456789abcdef0123456789abcdef"}),
            json!({"port":65536,"path":"0123456789abcdef0123456789abcdef"}),
            json!({"port":7681,"path":"../../other-host"}),
        ] {
            assert!(terminal_url(&base, &data).is_err());
        }
        let url = terminal_url(
            &base,
            &json!({"port":7681,"path":"0123456789abcdef0123456789abcdef"}),
        )
        .unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("box.local"));
    }
    #[test]
    fn shares_come_from_user_rights_and_include_personal_smb_folder() {
        let shares = user_shares(
            &json!({"home":{"smb":"alice"},"shares":[{"name":"Team","access":"ro","comment":"Lesen"}]}),
        );
        assert_eq!(shares.len(), 2);
        assert_eq!(shares[0].name, "alice");
        assert_eq!(shares[1].access, "ro");
    }
    #[test]
    fn backup_export_sends_options_only_when_set() {
        assert_eq!(export_args("", false, false), json!({"password":""}));
        assert_eq!(
            export_args("geheim1", true, false),
            json!({"password":"geheim1","certificate":true})
        );
        assert_eq!(
            export_args("geheim1", true, true),
            json!({"password":"geheim1","certificate":true,"ca":true})
        );
    }
    #[test]
    fn settings_backup_is_written_and_existing_files_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("settings.json");
        let content = r#"{"shareos_backup":1,"host":"box","encrypted":false}"#;
        save_settings_file(content, &target).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), content);
        assert!(save_settings_file(content, &target).is_err());
        assert!(save_settings_file("{}", &dir.path().join("invalid.json")).is_err());
        assert!(!dir.path().join("invalid.json").exists());
    }
    #[test]
    fn binding_failure_does_not_grant_admin_access() {
        let (address, server) = mock(vec![
            reply(json!({"ubus_rpc_session":"abc"})),
            reply(json!({"error":"session not bound"})),
            reply(json!({})),
        ]);
        assert!(Nas::connect(&address, "alice".into(), "test-password", None).is_err());
        let calls = server.join().unwrap();
        assert_eq!(calls[2]["params"][2], "destroy");
    }
    #[test]
    fn incomplete_directory_listing_never_produces_completed_backup() {
        let (address, server) = mock(vec![
            reply(json!({"ubus_rpc_session":"abc"})),
            reply(json!({"role":"user"})),
            reply(json!({"entries":[],"truncated":true})),
        ]);
        let nas = Nas::connect(&address, "alice".into(), "test-password", None).unwrap();
        let dir = tempfile::tempdir().unwrap();
        assert!(crate::backup::create(
            &nas,
            vec![Root {
                name: "Team".into(),
                home: false,
                ro: true
            }],
            dir.path()
        )
        .is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        server.join().unwrap();
    }
}

#[cfg(test)]
mod transfer_tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::Arc,
        thread,
    };
    type Request = (String, Vec<u8>);
    fn mock(responses: Vec<(Vec<u8>, usize)>) -> (Nas, thread::JoinHandle<Vec<Request>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = base_url(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let handle = thread::spawn(move || {
            let mut requests = vec![];
            for (body, advertised) in responses {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut header = vec![];
                while !header.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    socket.read_exact(&mut byte).unwrap();
                    header.push(byte[0]);
                }
                let header = String::from_utf8(header).unwrap();
                let length = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                let mut input = vec![0; length];
                socket.read_exact(&mut input).unwrap();
                requests.push((header, input));
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {advertised}\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
                socket.write_all(&body).unwrap();
            }
            requests
        });
        let nas = Nas {
            base,
            http: Http::builder().no_proxy().build().unwrap(),
            sid: "test".into(),
            user: "alice".into(),
            admin: false,
            shares: vec![],
            trusted: None,
        };
        (nas, handle)
    }
    fn ticket() -> (Vec<u8>, usize) {
        let bytes = json!({"result":[0,{"url":"/cgi-bin/files?t=test"}]})
            .to_string()
            .into_bytes();
        let size = bytes.len();
        (bytes, size)
    }
    #[test]
    fn upload_sends_known_length_and_reports_transferred_bytes() {
        let data = vec![42u8; 130_000];
        let reply = b"{\"ok\":true}".to_vec();
        let (nas, server) = mock(vec![ticket(), (reply.clone(), reply.len())]);
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("example.bin");
        std::fs::write(&source, &data).unwrap();
        let progress = Arc::new(crate::transfer::Progress::default());
        nas.upload_progress("Team", "", &source, progress.clone())
            .unwrap();
        assert_eq!(progress.values(), (130_000, 130_000));
        let requests = server.join().unwrap();
        assert_eq!(requests[1].1, data);
        assert!(requests[1]
            .0
            .to_lowercase()
            .contains("content-length: 130000"));
        let ticket: Value = serde_json::from_slice(&requests[0].1).unwrap();
        assert_eq!(ticket["params"][3]["overwrite"], false);
    }
    fn rpc_response(value: Value) -> (Vec<u8>, usize) {
        let bytes = json!({"result":[0,value]}).to_string().into_bytes();
        let size = bytes.len();
        (bytes, size)
    }
    #[test]
    fn file_drop_creates_folder_only_in_personal_home_and_uploads_there() {
        let (nas, server) = mock(vec![
            rpc_response(
                json!({"roots":[{"name":"Team","home":false},{"name":"home","home":true}]}),
            ),
            rpc_response(json!({"entries":[]})),
            rpc_response(json!({"ok":true})),
            rpc_response(json!({"entries":[]})),
            ticket(),
            (b"{\"ok\":true}".to_vec(), 11),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("note.txt");
        std::fs::write(&source, b"hello").unwrap();
        let report = crate::file_drop::upload(&nas, vec![source]).unwrap();
        assert_eq!(report.uploaded, 1);
        assert!(report.failures.is_empty());
        let requests = server.join().unwrap();
        let mkdir: Value = serde_json::from_slice(&requests[2].1).unwrap();
        assert_eq!(mkdir["params"][2], "mkdir");
        assert_eq!(mkdir["params"][3]["root"], "home");
        assert_eq!(mkdir["params"][3]["name"], "File Drop");
        let upload: Value = serde_json::from_slice(&requests[4].1).unwrap();
        assert_eq!(upload["params"][3]["root"], "home");
        assert_eq!(upload["params"][3]["path"], "File Drop");
        assert_eq!(upload["params"][3]["overwrite"], false);
        assert_eq!(requests[5].1, b"hello");
    }
    #[test]
    fn file_drop_reuses_existing_folder_and_renames_duplicate() {
        let (nas, server) = mock(vec![
            rpc_response(json!({"roots":[{"name":"home","home":true}]})),
            rpc_response(json!({"entries":[{"name":"File Drop","type":"dir","size":0}]})),
            rpc_response(json!({"entries":[{"name":"note.txt","type":"file","size":5}]})),
            ticket(),
            (b"{\"ok\":true}".to_vec(), 11),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("note.txt");
        std::fs::write(&source, b"hello").unwrap();
        assert_eq!(
            crate::file_drop::upload(&nas, vec![source])
                .unwrap()
                .uploaded,
            1
        );
        let requests = server.join().unwrap();
        let upload: Value = serde_json::from_slice(&requests[3].1).unwrap();
        assert_eq!(upload["params"][3]["name"], "note (2).txt");
        assert_eq!(requests.len(), 5);
    }
    #[test]
    fn file_drop_never_falls_back_to_a_shared_root_or_read_only_home() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("note.txt");
        std::fs::write(&source, b"hello").unwrap();
        for roots in [
            json!([{"name":"Team","home":false}]),
            json!([{"name":"home","home":true,"ro":true}]),
        ] {
            let (nas, server) = mock(vec![rpc_response(json!({"roots":roots}))]);
            assert!(crate::file_drop::upload(&nas, vec![source.clone()]).is_err());
            assert_eq!(server.join().unwrap().len(), 1);
        }
    }
    #[test]
    fn file_drop_rejects_a_file_named_like_the_destination_folder() {
        let (nas, server) = mock(vec![
            rpc_response(json!({"roots":[{"name":"home","home":true}]})),
            rpc_response(json!({"entries":[{"name":"File Drop","type":"file","size":5}]})),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("note.txt");
        std::fs::write(&source, b"hello").unwrap();
        assert!(crate::file_drop::upload(&nas, vec![source]).is_err());
        assert_eq!(server.join().unwrap().len(), 2);
    }
    #[test]
    fn file_operations_use_server_permissions_and_do_not_overwrite_or_force_delete() {
        let (nas, server) = mock(vec![
            rpc_response(json!({"ok":true})),
            rpc_response(json!({"ok":true})),
            rpc_response(json!({"ok":true})),
            ticket(),
            (b"{\"ok\":true}".to_vec(), 11),
        ]);
        nas.rename("Team", "folder", "old.txt", "new.txt").unwrap();
        nas.delete("Team", "folder", &["old.txt".into()]).unwrap();
        let clipboard = crate::file_ops::Clipboard::new(
            &nas,
            "Team".into(),
            "source".into(),
            vec!["file.txt".into()],
        );
        clipboard.paste(&nas, "Team", "dest").unwrap();
        nas.create_file("Team", "folder", "empty.txt").unwrap();
        let requests = server.join().unwrap();
        let params: Vec<Value> = requests[..4]
            .iter()
            .map(|request| serde_json::from_slice::<Value>(&request.1).unwrap()["params"].clone())
            .collect();
        assert_eq!(params[0][2], "rename");
        assert_eq!(params[1][3]["permanent"], false);
        assert_eq!(params[2][2], "move");
        assert_eq!(params[2][3]["copy"], true);
        assert_eq!(params[3][3]["overwrite"], false);
        assert!(requests[4].1.is_empty());
    }
    #[test]
    fn paste_between_roots_uses_read_and_write_tickets_without_overwrite() {
        let (nas, server) = mock(vec![
            rpc_response(json!({"entries":[{"name":"file.txt","type":"file","size":5}]})),
            rpc_response(json!({"entries":[]})),
            ticket(),
            (b"hello".to_vec(), 5),
            ticket(),
            (b"{\"ok\":true}".to_vec(), 11),
        ]);
        let clipboard = crate::file_ops::Clipboard::new(
            &nas,
            "Team".into(),
            "source".into(),
            vec!["file.txt".into()],
        );
        clipboard.paste(&nas, "home", "dest").unwrap();
        let requests = server.join().unwrap();
        let get: Value = serde_json::from_slice(&requests[2].1).unwrap();
        let put: Value = serde_json::from_slice(&requests[4].1).unwrap();
        assert_eq!(get["params"][3]["root"], "Team");
        assert_eq!(get["params"][3]["mode"], "get");
        assert_eq!(put["params"][3]["root"], "home");
        assert_eq!(put["params"][3]["path"], "dest");
        assert_eq!(put["params"][3]["overwrite"], false);
        assert_eq!(requests[5].1, b"hello");
    }
    #[test]
    fn cross_root_folder_copy_preserves_empty_directories_and_places_nested_files() {
        let (nas, server) = mock(vec![
            rpc_response(json!({"entries":[{"name":"folder","type":"dir","size":0}]})),
            rpc_response(json!({"entries":[]})),
            rpc_response(
                json!({"entries":[{"name":"empty","type":"dir","size":0},{"name":"file.txt","type":"file","size":5}]}),
            ),
            rpc_response(json!({"entries":[]})),
            rpc_response(json!({"ok":true})),
            rpc_response(json!({"ok":true})),
            ticket(),
            (b"hello".to_vec(), 5),
            ticket(),
            (b"{\"ok\":true}".to_vec(), 11),
        ]);
        let clipboard = crate::file_ops::Clipboard::new(
            &nas,
            "Team".into(),
            "source".into(),
            vec!["folder".into()],
        );
        clipboard.paste(&nas, "home", "dest").unwrap();
        let requests = server.join().unwrap();
        let mkdir: Value = serde_json::from_slice(&requests[4].1).unwrap();
        let empty: Value = serde_json::from_slice(&requests[5].1).unwrap();
        let put: Value = serde_json::from_slice(&requests[8].1).unwrap();
        assert_eq!(mkdir["params"][3]["path"], "dest");
        assert_eq!(mkdir["params"][3]["name"], "folder");
        assert_eq!(empty["params"][3]["path"], "dest/folder");
        assert_eq!(empty["params"][3]["name"], "empty");
        assert_eq!(put["params"][3]["path"], "dest/folder");
    }
    #[test]
    fn cross_root_copy_rejects_existing_destination_before_writes() {
        let entries = json!({"entries":[{"name":"file.txt","type":"file","size":5}]});
        let (nas, server) = mock(vec![rpc_response(entries.clone()), rpc_response(entries)]);
        let clipboard = crate::file_ops::Clipboard::new(
            &nas,
            "Team".into(),
            "source".into(),
            vec!["file.txt".into()],
        );
        assert!(clipboard.paste(&nas, "home", "dest").is_err());
        assert_eq!(server.join().unwrap().len(), 2);
    }
    #[test]
    fn interrupted_download_removes_partial_file() {
        let (nas, server) = mock(vec![ticket(), (b"partial".to_vec(), 100)]);
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("file.bin");
        let progress = crate::transfer::Progress::default();
        assert!(nas
            .download_progress("Team", "", "file.bin", &target, &progress)
            .is_err());
        assert!(!target.exists());
        server.join().unwrap();
    }
}

pub fn save_settings_file(content: &str, target: &Path) -> Result<()> {
    use std::io::Write;
    let value: Value =
        serde_json::from_str(content).context("Ungültige Sicherungsdaten von der Box")?;
    ensure!(
        value["shareos_backup"] == 1 || value["shareos_backup"] == 2,
        "Keine ShareOS-Einstellungssicherung erhalten"
    );
    let mut file = private_file(target)
        .context("Sicherungsdatei konnte nicht angelegt werden. Einen neuen Dateinamen wählen.")?;
    let result = (|| -> Result<()> {
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        drop(file);
        let _ = std::fs::remove_file(target);
    }
    result
}
