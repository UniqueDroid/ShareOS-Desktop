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
        }
    }

    pub fn connect(
        address: &str,
        user: String,
        password: &str,
        trusted: Option<&crate::tls::Certificate>,
    ) -> Result<Self> {
        let base = base_url(address)?;
        let mut nas = Self {
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
            user,
            admin: false,
        };
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
            Ok(me) => nas.admin = me["role"].as_str() == Some("admin"),
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
            crate::backup::safe_name(name),
            "Gültigen Ordnernamen angeben"
        );
        self.rpc(
            "shareos.files",
            "mkdir",
            json!({"root":root,"path":path,"name":name}),
        )?;
        Ok(())
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
    pub fn settings(&self, password: &str) -> Result<Value> {
        ensure!(self.admin, "Administratorrechte erforderlich");
        self.rpc("shareos.backup", "export", json!({"password":password}))
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
// Public identification only: discovery never sends credentials or trusts a
// discovered certificate for subsequent login.
pub fn discover(prefix: &str, configured: &str) -> Result<Vec<String>> {
    let addresses = discovery_addresses(prefix, configured)?;
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
                        probe(http, address)
                            .ok()
                            .filter(|found| *found)
                            .map(|_| address.clone())
                    })
                })
                .collect();
            for job in jobs {
                if let Ok(Some(address)) = job.join() {
                    if !found.contains(&address) {
                        found.push(address);
                    }
                }
            }
        });
    }
    Ok(found)
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
        assert!(nas.settings("").is_err());
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
