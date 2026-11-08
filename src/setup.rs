//! "Box einrichten": the first setup of a new ShareOS box, and the switch of
//! an Alt-F box to ShareOS (see altf.rs), from the desktop – like the vendor's
//! setup tool. Everything goes through the box's own interfaces:
//! `shareos.setup` (password, name, time zone, update channel, Alt-F data),
//! `shareos.storage` (take disks over as they are, or set them up new),
//! `shareos.accounts` / `shareos.shares` (first user and share).
//! The box checks every value itself; formatting needs an explicit
//! confirmation here and on the box (`confirm: true`).
use crate::{altf, nas::Nas, theme::Palette, tls};
use anyhow::{bail, ensure, Context, Result};
use eframe::egui;
use serde_json::{json, Value};
use std::{
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

/// file systems a partition can be taken over with (lsblk FSTYPE)
const ADOPTABLE: [&str; 8] = [
    "ext4", "ext3", "ext2", "btrfs", "xfs", "vfat", "exfat", "ntfs",
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Stage {
    Start,
    /// ShareOS answers, the default password did not work: ask for it
    ShareOsPassword,
    Basics,
    Disks,
    DiskJob,
    User,
    Done,
    AltLogin,
    AltFirmware,
    AltConfirm,
    AltFlashing,
    /// HTTPS with an unknown (self-signed) certificate: confirm it first
    Certificate,
    /// a new box keeps root locked: USB button, then the first password
    Claim,
}

/// Results of the background jobs; a job can send several (progress lines).
enum Step {
    Log(String),
    ShareOs,
    AltF,
    Unknown,
    /// the box waits to be claimed (root locked)
    Claim,
    LoggedIn(Box<Nas>, Value, Vec<String>, Value),
    /// the box certificate is not trusted yet
    Untrusted(tls::Certificate),
    /// logged in with the factory password "shareos"
    Default(Box<Step>),
    Applied(Vec<String>),
    Storage(Value),
    UserDone(Vec<String>),
    AltLoggedIn(Box<altf::AltF>),
    Firmware(Box<altf::Firmware>),
    Uploaded(altf::Checked),
    /// ShareOS answers at the address after the switch
    Switched,
}

struct Adopt {
    part: String,
    fs: String,
    size: u64,
    take: bool,
    name: String,
}

/// (volume name, partition) pairs to take over, and the arguments of a new setup
type DiskJobs = (Vec<(String, String)>, Option<Value>);

/// What the main window should do after the wizard.
pub enum Outcome {
    None,
    /// open "Verbinden" with this address and user
    Login(String, String, Option<tls::Certificate>),
}

pub struct Setup {
    pub address: String,
    stage: Stage,
    job: Option<Receiver<Result<Step>>>,
    log: Vec<String>,
    error: String,
    // ShareOS
    nas: Option<Nas>,
    root_password: String,
    info: Value,
    zones: Vec<String>,
    altf_data: Value,
    password: String,
    password2: String,
    hostname: String,
    timezone: String,
    beta: bool,
    import_altf: bool,
    storage: Value,
    adopt: Vec<Adopt>,
    format: bool,
    format_mirror: bool,
    format_disks: Vec<(String, u64, bool)>,
    format_name: String,
    format_confirm: String,
    user: String,
    user_password: String,
    user_password2: String,
    share: bool,
    done: Vec<String>,
    // Alt-F
    alt: Option<altf::AltF>,
    alt_password: String,
    model: usize,
    firmware: Option<altf::Firmware>,
    checked: Option<altf::Checked>,
    understood: bool,
    started: Option<Instant>,
    /// still the factory password: a new one is required
    default_password: bool,
    claim_password: String,
    claim_repeat: String,
    /// update channel chosen for the switch from Alt-F
    keep_beta: Option<bool>,
    /// check the box as soon as the tab is shown (opened from the search)
    auto_detect: bool,
    /// confirmed certificate of an HTTPS box (pinned like in "Verbinden")
    trusted: Option<tls::Certificate>,
    certificate: Option<tls::Certificate>,
}

impl Setup {
    pub fn new(address: &str) -> Self {
        Self {
            address: address.into(),
            stage: Stage::Start,
            job: None,
            log: vec![],
            error: String::new(),
            nas: None,
            root_password: String::new(),
            info: Value::Null,
            zones: vec![],
            altf_data: Value::Null,
            password: String::new(),
            password2: String::new(),
            hostname: String::new(),
            timezone: "Europe/Berlin".into(),
            beta: true,
            import_altf: true,
            storage: Value::Null,
            adopt: vec![],
            format: false,
            format_mirror: false,
            format_disks: vec![],
            format_name: "daten".into(),
            format_confirm: String::new(),
            user: String::new(),
            user_password: String::new(),
            user_password2: String::new(),
            share: true,
            done: vec![],
            alt: None,
            alt_password: String::new(),
            model: 0,
            firmware: None,
            checked: None,
            understood: false,
            started: None,
            default_password: false,
            claim_password: String::new(),
            claim_repeat: String::new(),
            keep_beta: None,
            auto_detect: false,
            trusted: None,
            certificate: None,
        }
    }
    /// Opened from the box search: check the address right away.
    pub fn detected(address: &str) -> Self {
        Self {
            auto_detect: true,
            ..Self::new(address)
        }
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    fn run(
        &mut self,
        ctx: &egui::Context,
        job: impl FnOnce(&Sender<Result<Step>>) -> Result<Step> + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        self.error.clear();
        self.job = Some(rx);
        std::thread::spawn(move || {
            let result = job(&tx);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.job else { return };
        let mut finished = None;
        loop {
            match rx.try_recv() {
                Ok(Ok(Step::Log(line))) => self.log.push(line),
                Ok(other) => {
                    finished = Some(other);
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished = Some(Err(anyhow::anyhow!("Aufgabe abgebrochen")));
                    break;
                }
            }
        }
        if self.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        let Some(result) = finished else { return };
        self.job = None;
        match result {
            Err(e) => self.error = format!("{e:#}"),
            Ok(step) => self.apply(step, ctx),
        }
    }
    fn apply(&mut self, step: Step, ctx: &egui::Context) {
        match step {
            Step::Log(line) => self.log.push(line),
            Step::Unknown => {
                self.error = "Unter dieser Adresse antwortet weder ShareOS noch Alt-F. Adresse prüfen (bei Alt-F http:// verwenden).".into()
            }
            Step::ShareOs => self.stage = Stage::ShareOsPassword,
            Step::Claim => {
                self.claim_password.clear();
                self.claim_repeat.clear();
                self.stage = Stage::Claim;
            }
            Step::AltF => self.stage = Stage::AltLogin,
            Step::Untrusted(cert) => {
                self.certificate = Some(cert);
                self.stage = Stage::Certificate;
            }
            Step::Default(inner) => {
                self.default_password = true;
                self.apply(*inner, ctx);
            }
            Step::LoggedIn(nas, info, zones, altf_data) => {
                self.hostname = info["hostname"].as_str().unwrap_or("shareos").into();
                if let Some(tz) = info["timezone"].as_str().filter(|z| !z.is_empty() && *z != "UTC") {
                    self.timezone = tz.into();
                }
                self.beta = match self.keep_beta.take() {
                    Some(beta) => beta,
                    None => info["channel"].as_str() != Some("stable"),
                };
                self.import_altf = altf_data["found"].as_bool() == Some(true);
                self.info = info;
                self.zones = zones;
                self.altf_data = altf_data;
                self.nas = Some(*nas);
                self.stage = Stage::Basics;
                if self.info["done"].as_bool() == Some(true) {
                    self.log.push("Die Box ist schon eingerichtet – der Assistent kann Einstellungen trotzdem ändern.".into());
                }
            }
            Step::Applied(done) => {
                self.default_password = false;
                self.log.push(if done.is_empty() {
                    "Grundeinstellungen übernommen.".into()
                } else {
                    format!("Grundeinstellungen übernommen: {}", done.join(", "))
                });
                self.load_storage(ctx);
            }
            Step::Storage(st) => {
                let running = st["job"]["state"].as_str() == Some("running");
                self.plan_disks(&st);
                self.storage = st;
                if self.stage == Stage::DiskJob && !running {
                    match self.storage["job"]["state"].as_str() {
                        Some("error") => {
                            self.error = format!(
                                "Einrichten der Platten fehlgeschlagen: {}",
                                self.storage["job"]["error"].as_str().unwrap_or("unbekannter Fehler")
                            );
                            self.stage = Stage::Disks;
                        }
                        _ => {
                            self.log.push("Platten eingerichtet.".into());
                            self.stage = Stage::User;
                        }
                    }
                } else if self.stage == Stage::DiskJob {
                    // ask again in a few seconds
                    let nas = self.nas.clone();
                    self.run(ctx, move |_| {
                        std::thread::sleep(Duration::from_secs(4));
                        Ok(Step::Storage(nas.context("nicht angemeldet")?.rpc("shareos.storage", "status", json!({}))?))
                    });
                } else if self.stage == Stage::Basics || self.stage == Stage::Disks {
                    self.stage = Stage::Disks;
                }
            }
            Step::UserDone(lines) => {
                self.done.extend(lines);
                self.stage = Stage::Done;
            }
            Step::AltLoggedIn(alt) => {
                self.alt = Some(*alt);
                self.stage = Stage::AltFirmware;
            }
            Step::Firmware(fw) => {
                self.log.push(format!(
                    "ShareOS {} für {} geladen ({} KB), SHA-256 und SHA-1 geprüft.",
                    fw.version,
                    fw.model,
                    fw.bin.len() / 1024
                ));
                self.firmware = Some(*fw);
                self.upload(ctx);
            }
            Step::Uploaded(checked) => {
                self.checked = Some(checked);
                self.understood = false;
                self.stage = Stage::AltConfirm;
            }
            Step::Switched => {
                self.log.push("ShareOS läuft. Weiter mit der Einrichtung.".into());
                self.alt = None;
                self.firmware = None;
                self.checked = None;
                self.keep_beta = Some(self.beta);
                self.stage = Stage::Start;
                // new firmware: claim with the USB button; older: password "shareos"
                self.detect(ctx);
            }
        }
    }

    /* ---------- jobs ---------- */

    /// the confirmed certificate, only for the address it was confirmed for
    fn trust(&self) -> Option<tls::Certificate> {
        let origin = crate::nas::base_url(&self.address)
            .ok()?
            .origin()
            .ascii_serialization();
        self.trusted.clone().filter(|cert| cert.origin == origin)
    }
    fn detect(&mut self, ctx: &egui::Context) {
        let address = self.address.clone();
        let trusted = self.trust();
        self.log.clear();
        self.default_password = false;
        self.run(ctx, move |_| {
            let base = crate::nas::base_url(&address)?;
            let http = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(5))
                .danger_accept_invalid_certs(true)
                .redirect(reqwest::redirect::Policy::none())
                .build()?;
            if crate::nas::probe(&http, base.as_str()).unwrap_or(false) {
                // a new box (0.3.0-beta.2+) keeps root locked until it is claimed
                match Nas::anonymous(&address, trusted.as_ref())?.claim_state() {
                    Ok(Some(_)) => return Ok(Step::Claim),
                    Ok(None) => {}
                    Err(e) if trusted.is_none() && tls::is_certificate_error(&e) => {
                        return Ok(Step::Untrusted(tls::inspect(&address)?))
                    }
                    Err(e) => return Err(e),
                }
                // older firmware: a new box still has the password "shareos"
                return match Nas::connect(&address, "root".into(), "shareos", trusted.as_ref()) {
                    Ok(nas) => Ok(Step::Default(Box::new(logged_in(nas)?))),
                    Err(e) if trusted.is_none() && tls::is_certificate_error(&e) => {
                        Ok(Step::Untrusted(tls::inspect(&address)?))
                    }
                    Err(_) => Ok(Step::ShareOs),
                };
            }
            if altf::detect(&http, &base).unwrap_or(false) {
                return Ok(Step::AltF);
            }
            Ok(Step::Unknown)
        });
    }
    /// first password of a new box, then log in with it
    fn claim(&mut self, ctx: &egui::Context) {
        let address = self.address.clone();
        let trusted = self.trust();
        let password = self.claim_password.clone();
        self.root_password = password.clone();
        self.default_password = false;
        self.run(ctx, move |_| {
            Nas::anonymous(&address, trusted.as_ref())?.claim(&password)?;
            logged_in(Nas::connect(
                &address,
                "root".into(),
                &password,
                trusted.as_ref(),
            )?)
        });
    }
    fn login(&mut self, ctx: &egui::Context) {
        let address = self.address.clone();
        let trusted = self.trust();
        let password = self.root_password.clone();
        self.default_password = password == "shareos";
        self.run(ctx, move |_| {
            match Nas::connect(&address, "root".into(), &password, trusted.as_ref()) {
                Ok(nas) => logged_in(nas),
                Err(e) if trusted.is_none() && tls::is_certificate_error(&e) => {
                    Ok(Step::Untrusted(tls::inspect(&address)?))
                }
                Err(e) => Err(e),
            }
        });
    }
    fn load_storage(&mut self, ctx: &egui::Context) {
        let nas = self.nas.clone();
        self.run(ctx, move |_| {
            Ok(Step::Storage(nas.context("nicht angemeldet")?.rpc(
                "shareos.storage",
                "status",
                json!({}),
            )?))
        });
    }
    fn plan_disks(&mut self, st: &Value) {
        let used: Vec<String> = st["volumes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| {
                v["device"]
                    .as_str()
                    .map(|d| d.trim_start_matches("/dev/").to_string())
            })
            .collect();
        let mut adopt = vec![];
        let mut disks = vec![];
        for d in st["disks"].as_array().into_iter().flatten() {
            if d["usb"].as_bool() == Some(true) {
                continue;
            }
            let name = d["name"].as_str().unwrap_or("").to_string();
            disks.push((name, d["size"].as_u64().unwrap_or(0), true));
            collect_parts(d, &used, &mut adopt);
        }
        for (i, a) in adopt.iter_mut().enumerate() {
            a.name = if i == 0 {
                "daten".into()
            } else {
                format!("daten{}", i + 1)
            };
        }
        // keep what the user already changed when the list is the same
        if self
            .adopt
            .iter()
            .map(|a| &a.part)
            .ne(adopt.iter().map(|a| &a.part))
        {
            self.adopt = adopt;
        }
        if self
            .format_disks
            .iter()
            .map(|d| &d.0)
            .ne(disks.iter().map(|d| &d.0))
        {
            self.format_mirror = disks.len() >= 2;
            self.format_disks = disks;
        }
    }
    fn upload(&mut self, ctx: &egui::Context) {
        let (Some(alt), Some(fw)) = (self.alt.clone(), self.firmware.as_ref()) else {
            return;
        };
        let (name, bin, sha1) = (fw.bin_name.clone(), fw.bin.clone(), fw.sha1.clone());
        self.run(ctx, move |tx| {
            let _ = tx.send(Ok(Step::Log(
                "Firmware wird zu Alt-F hochgeladen und dort geprüft …".into(),
            )));
            Ok(Step::Uploaded(alt.upload(&name, &bin, &sha1)?))
        });
    }
    /// The point of no return: special reboot if Alt-F wants one, flash, reboot,
    /// wait for ShareOS (it installs its system itself, from a USB stick or GitHub).
    fn switch(&mut self, ctx: &egui::Context) {
        let (Some(alt), Some(fw)) = (self.alt.clone(), self.firmware.as_ref()) else {
            return;
        };
        let special = matches!(self.checked, Some(altf::Checked::NeedsSpecialReboot(_)));
        let (name, bin, sha1) = (fw.bin_name.clone(), fw.bin.clone(), fw.sha1.clone());
        let password = self.alt_password.clone();
        self.stage = Stage::AltFlashing;
        self.started = Some(Instant::now());
        self.run(ctx, move |tx| {
            let log = |s: &str| {
                let _ = tx.send(Ok(Step::Log(s.into())));
            };
            let mut alt = alt;
            if special {
                log("Alt-F läuft aus dem Flash: zuerst Spezial-Neustart …");
                alt.flash("SpecialReboot")?;
                alt.reboot()?;
                std::thread::sleep(Duration::from_secs(30));
                alt = wait_for(Duration::from_secs(600), || altf::AltF::login(&alt.base, &password))
                    .context("Alt-F kommt nach dem Spezial-Neustart nicht zurück")?;
                log("Alt-F ist wieder da – Firmware erneut hochladen …");
                match alt.upload(&name, &bin, &sha1)? {
                    altf::Checked::Ready(_) => {}
                    other => bail!("Alt-F bietet das Flashen nach dem Spezial-Neustart nicht an: {other:?}"),
                }
            }
            log("Flashen läuft (Kernel und Startsystem, 1–3 Minuten). Box NICHT ausschalten …");
            alt.flash("FlashIt")?;
            log("Geflasht und geprüft. Neustart …");
            alt.reboot()?;
            std::thread::sleep(Duration::from_secs(45));
            log("ShareOS installiert jetzt sein System (vom USB-Stick oder von GitHub, einige Minuten; die orange USB-LED blinkt schnell) …");
            let http = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(5))
                .danger_accept_invalid_certs(true)
                .redirect(reqwest::redirect::Policy::none())
                .build()?;
            wait_for(Duration::from_secs(1800), || {
                ensure!(crate::nas::probe(&http, alt.base.as_str())?, "noch nicht");
                Ok(())
            })
            .context("ShareOS meldet sich nicht. Blinkt die orange USB-LED langsam, steckt die Box im Rettungsmodus: die .sysupgrade.tar auf einen USB-Stick kopieren, einstecken, Box aus- und einschalten (siehe UMSTIEG.md im ShareOS-Repository).")?;
            Ok(Step::Switched)
        });
    }

    /* ---------- UI ---------- */

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        p: &Palette,
        trusted: Option<tls::Certificate>,
    ) -> Outcome {
        self.poll(ctx);
        // a certificate already confirmed in "Verbinden" counts here too
        if self.trusted.is_none() {
            self.trusted = trusted;
        }
        if self.auto_detect && self.stage == Stage::Start && !self.busy() {
            self.auto_detect = false;
            self.detect(ctx);
        }
        let mut outcome = Outcome::None;
        text(
            ui,
            "Eine neue Box einrichten oder eine Box mit Alt-F auf ShareOS umstellen.",
        );
        ui.horizontal(|ui| {
            ui.label("Box-Adresse");
            ui.add_enabled(
                matches!(self.stage, Stage::Start) && !self.busy(),
                crate::text_input(&mut self.address).desired_width(320.0),
            );
            if self.stage != Stage::Start
                && !matches!(self.stage, Stage::AltFlashing)
                && !self.busy()
                && ui.button("Von vorn").clicked()
            {
                *self = Setup::new(&self.address.clone());
            }
        });
        ui.add_space(6.0);
        ui.add_enabled_ui(!self.busy(), |ui| match self.stage {
            Stage::Start => {
                small(ui, "Neue Boxen mit ShareOS übernimmt man mit der USB-Taste an der Box (ältere Versionen: Passwort „shareos“); Alt-F-Boxen über http://.");
                if ui.button("Box prüfen").clicked() {
                    self.detect(ctx);
                }
            }
            Stage::Claim => {
                text(ui, "Neue Box: Sie hat noch kein Passwort und lässt sich erst übernehmen, wenn jemand direkt an der Box steht.");
                text(ui, "1. Die USB-Taste vorne an der Box kurz drücken.");
                text(ui, "2. Innerhalb von 2 Minuten hier das Passwort für den Administrator (root) wählen:");
                ui.horizontal(|ui| {
                    ui.label("Neues Passwort");
                    ui.add(crate::text_input(&mut self.claim_password).password(true).desired_width(220.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Wiederholen");
                    ui.add(crate::text_input(&mut self.claim_repeat).password(true).desired_width(220.0));
                });
                let ok = self.claim_password.chars().count() >= 8 && self.claim_password == self.claim_repeat;
                if !self.claim_password.is_empty() && !ok {
                    small(ui, if self.claim_password.chars().count() < 8 { "Mindestens 8 Zeichen." } else { "Die Passwörter sind nicht gleich." });
                }
                if ui.add_enabled(ok, egui::Button::new("Box übernehmen")).clicked() {
                    self.claim(ctx);
                }
            }
            Stage::ShareOsPassword => {
                text(ui, "ShareOS gefunden. Das Admin-Passwort ist schon geändert – zum Einrichten mit root anmelden:");
                ui.horizontal(|ui| {
                    ui.label("Passwort von root");
                    ui.add(crate::text_input(&mut self.root_password).password(true).desired_width(240.0));
                });
                if ui.button("Anmelden").clicked() {
                    self.login(ctx);
                }
            }
            Stage::Basics => self.basics_ui(ui, ctx),
            Stage::Disks => self.disks_ui(ui, ctx, p),
            Stage::DiskJob => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    text(ui, format!(
                        "Platten werden eingerichtet – {}. Box nicht ausschalten.",
                        self.storage["job"]["step"].as_str().unwrap_or("bitte warten")
                    ));
                });
            }
            Stage::User => self.user_ui(ui, ctx),
            Stage::Done => {
                ui.label(egui::RichText::new("Die Box ist eingerichtet.").strong().color(p.accent));
                for line in &self.done {
                    text(ui, format!("• {line}"));
                }
                small(ui, "Mit dem neuen Benutzer anmelden, um Dateien zu sehen und das Netzlaufwerk zu verbinden.");
                if ui.button("Zur Anmeldung").clicked() {
                    let user = if self.user.is_empty() { "root".to_string() } else { self.user.clone() };
                    outcome = Outcome::Login(self.address.clone(), user, self.trust());
                }
            }
            Stage::Certificate => {
                if let Some(cert) = self.certificate.clone() {
                    text(ui, "Die Box verwendet ein eigenes HTTPS-Zertifikat. Fingerabdruck mit der Box vergleichen (Browser: Schloss-Symbol → Zertifikat) und dann vertrauen:");
                    ui.add(
                        egui::Label::new(egui::RichText::new(cert.fingerprint()).monospace()).wrap(),
                    );
                    small(ui, "Das Vertrauen gilt nur für diese Adresse und genau dieses Zertifikat. Alternativ die Box über http:// einrichten.");
                    if ui.button("Zertifikat vertrauen und weiter").clicked() {
                        self.trusted = Some(cert);
                        self.certificate = None;
                        self.stage = Stage::Start;
                        self.detect(ctx);
                    }
                }
            }
            Stage::AltLogin => {
                text(ui, "Alt-F gefunden. Mit dem Passwort der Alt-F-Weboberfläche anmelden:");
                ui.horizontal(|ui| {
                    ui.label("Alt-F-Passwort");
                    ui.add(crate::text_input(&mut self.alt_password).password(true).desired_width(240.0));
                });
                if ui.button("Bei Alt-F anmelden").clicked() {
                    let password = self.alt_password.clone();
                    let address = self.address.clone();
                    self.run(ctx, move |_| {
                        let base = crate::nas::base_url(&address)?;
                        Ok(Step::AltLoggedIn(Box::new(altf::AltF::login(&base, &password)?)))
                    });
                }
            }
            Stage::AltFirmware => {
                text(ui, "Welches Gerät ist es? Alt-F prüft das Modell zusätzlich selbst.");
                egui::ComboBox::from_id_salt("model")
                    .selected_text(altf::MODELS[self.model].1)
                    .show_ui(ui, |ui| {
                        for (i, m) in altf::MODELS.iter().enumerate() {
                            ui.selectable_value(&mut self.model, i, m.1);
                        }
                    });
                ui.checkbox(&mut self.beta, "Auch Testversionen (Beta)");
                small(ui, "Lädt das neueste ShareOS von GitHub und prüft SHA-256 und SHA-1. Noch wird nichts geflasht.");
                if ui.button("Firmware laden und von Alt-F prüfen lassen").clicked() {
                    let model = altf::MODELS[self.model].0.to_string();
                    let beta = self.beta;
                    self.run(ctx, move |tx| {
                        let _ = tx.send(Ok(Step::Log("Firmware wird von GitHub geladen …".into())));
                        Ok(Step::Firmware(Box::new(altf::download(&model, beta)?)))
                    });
                }
            }
            Stage::AltConfirm => self.confirm_ui(ui, ctx, p),
            Stage::AltFlashing => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    let secs = self.started.map(|s| s.elapsed().as_secs()).unwrap_or(0);
                    text(ui, format!("Umstellung läuft seit {}:{:02} min – Box nicht ausschalten.", secs / 60, secs % 60));
                });
            }
        });
        if self.busy() && !matches!(self.stage, Stage::AltFlashing | Stage::DiskJob) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Bitte warten …");
            });
        }
        if !self.error.is_empty() {
            ui.add_space(6.0);
            warn(ui, p.error, format!("Fehler: {}", self.error));
        }
        if !self.log.is_empty() {
            ui.add_space(8.0);
            ui.separator();
            for line in self.log.iter().rev().take(12).rev() {
                small(ui, line);
            }
        }
        outcome
    }

    fn basics_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(egui::RichText::new("1 · Grundeinstellungen").strong());
        ui.horizontal(|ui| {
            ui.label("Neues Admin-Passwort (root)");
            ui.add(
                crate::text_input(&mut self.password)
                    .password(true)
                    .desired_width(200.0),
            );
            ui.label("wiederholen");
            ui.add(
                crate::text_input(&mut self.password2)
                    .password(true)
                    .desired_width(200.0),
            );
        });
        if self.default_password {
            let color = ui.visuals().warn_fg_color;
            warn(ui, color, "Die Box hat noch das Werks-Passwort „shareos“ – bitte ein eigenes festlegen (mindestens 6 Zeichen).");
        } else {
            small(
                ui,
                "Mindestens 6 Zeichen. Leer lassen, um das bisherige Passwort zu behalten.",
            );
        }
        crate::field(ui, "Name der Box", &mut self.hostname);
        ui.horizontal(|ui| {
            ui.label("Zeitzone");
            egui::ComboBox::from_id_salt("tz")
                .selected_text(&self.timezone)
                .height(320.0)
                .show_ui(ui, |ui| {
                    for z in &self.zones {
                        ui.selectable_value(&mut self.timezone, z.clone(), z);
                    }
                });
        });
        ui.checkbox(&mut self.beta, "Updates: auch Testversionen (Beta)");
        if self.altf_data["found"].as_bool() == Some(true) {
            ui.checkbox(
                &mut self.import_altf,
                format!(
                    "Von Alt-F übernehmen: {} Benutzer, {} Freigaben",
                    self.altf_data["users"].as_array().map_or(0, |a| a.len()),
                    self.altf_data["shares"].as_array().map_or(0, |a| a.len())
                ),
            );
        }
        if ui.button("Weiter").clicked() {
            match self.basics_args() {
                Err(e) => self.error = e.to_string(),
                Ok(args) => {
                    let nas = self.nas.clone();
                    self.password.clear();
                    self.password2.clear();
                    self.run(ctx, move |_| {
                        let r =
                            nas.context("nicht angemeldet")?
                                .rpc("shareos.setup", "apply", args)?;
                        Ok(Step::Applied(
                            r["done"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|x| x.as_str().map(String::from))
                                .collect(),
                        ))
                    });
                }
            }
        }
    }
    fn basics_args(&self) -> Result<Value> {
        ensure!(
            self.password == self.password2,
            "Die Passwörter stimmen nicht überein"
        );
        ensure!(
            self.password.is_empty() || self.password.chars().count() >= 6,
            "Passwort zu kurz (mindestens 6 Zeichen)"
        );
        ensure!(
            !self.default_password || !self.password.is_empty(),
            "Bitte ein eigenes Admin-Passwort festlegen (die Box hat noch „shareos“)"
        );
        ensure!(
            self.password != "shareos",
            "Bitte nicht wieder „shareos“ als Passwort verwenden"
        );
        ensure!(
            valid_hostname(&self.hostname),
            "Name: Buchstaben, Ziffern und -, höchstens 63 Zeichen"
        );
        Ok(json!({
            "password": self.password,
            "hostname": self.hostname,
            "timezone": self.timezone,
            "channel": if self.beta { "beta" } else { "stable" },
            "altf": self.import_altf && self.altf_data["found"].as_bool() == Some(true),
        }))
    }

    fn disks_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, p: &Palette) {
        ui.label(egui::RichText::new("2 · Platten").strong());
        let volumes: Vec<String> = self.storage["volumes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v["name"].as_str().map(String::from))
            .collect();
        if !volumes.is_empty() {
            text(
                ui,
                format!("Schon eingerichtete Laufwerke: {}", volumes.join(", ")),
            );
        }
        if !self.adopt.is_empty() {
            text(ui, "Diese Partitionen haben schon Daten. Sie werden so, wie sie sind, als Laufwerk eingebunden – es wird nichts gelöscht:");
            for a in &mut self.adopt {
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut a.take,
                        format!("{} ({}, {})", a.part, a.fs, crate::browser::size(a.size)),
                    );
                    ui.label("als");
                    ui.add_enabled(a.take, crate::text_input(&mut a.name).desired_width(140.0));
                });
            }
        }
        if self.format_disks.is_empty() {
            text(ui, "Keine Platte eingebaut (USB-Laufwerke richtet die Box selbst ein, wenn sie eingesteckt werden).");
        } else if self.adopt.is_empty() && volumes.is_empty() {
            ui.checkbox(
                &mut self.format,
                "Platten neu einrichten – ALLE Daten darauf werden gelöscht",
            );
            if self.format {
                for d in &mut self.format_disks {
                    ui.checkbox(&mut d.2, format!("{} ({})", d.0, crate::browser::size(d.1)));
                }
                let chosen = self.format_disks.iter().filter(|d| d.2).count();
                ui.add_enabled_ui(chosen == 2, |ui| {
                    ui.checkbox(
                        &mut self.format_mirror,
                        "Spiegeln (RAID 1): beide Platten enthalten dieselben Daten",
                    );
                });
                crate::field(ui, "Name des Laufwerks", &mut self.format_name);
                ui.horizontal(|ui| {
                    warn(ui, p.error, "Zum Bestätigen LÖSCHEN eintippen:");
                    ui.add(crate::text_input(&mut self.format_confirm).desired_width(120.0));
                });
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Weiter").clicked() {
                match self.disk_jobs() {
                    Err(e) => self.error = e.to_string(),
                    Ok((adopts, format)) => {
                        let nas = self.nas.clone();
                        let skip = adopts.is_empty() && format.is_none();
                        if !skip {
                            self.stage = Stage::DiskJob;
                        }
                        self.format_confirm.clear();
                        self.run(ctx, move |tx| {
                            let nas = nas.context("nicht angemeldet")?;
                            for (name, part) in adopts {
                                nas.rpc("shareos.storage", "adopt", json!({"name": name, "part": part}))
                                    .with_context(|| format!("{part} übernehmen"))?;
                                let _ = tx.send(Ok(Step::Log(format!("{part} ist jetzt das Laufwerk „{name}“ – die Daten sind erhalten."))));
                            }
                            if let Some(args) = format {
                                nas.rpc("shareos.storage", "setup", args)?;
                                let _ = tx.send(Ok(Step::Log("Platten werden eingerichtet …".into())));
                                std::thread::sleep(Duration::from_secs(3));
                            }
                            Ok(Step::Storage(nas.rpc("shareos.storage", "status", json!({}))?))
                        });
                        if skip {
                            self.stage = Stage::User;
                        }
                    }
                }
            }
            if ui.button("Platten jetzt nicht einrichten").clicked() {
                self.stage = Stage::User;
            }
        });
    }
    /// (name, partition) to take over, and the arguments of a new setup
    fn disk_jobs(&self) -> Result<DiskJobs> {
        let mut adopts = vec![];
        for a in self.adopt.iter().filter(|a| a.take) {
            let name = a.name.trim().to_ascii_lowercase();
            ensure!(
                valid_volume(&name),
                "Laufwerksname „{}“: a–z, 0–9, - und _, höchstens 16 Zeichen",
                a.name
            );
            ensure!(
                !adopts.iter().any(|(n, _): &(String, String)| *n == name),
                "Laufwerksname „{name}“ doppelt"
            );
            adopts.push((name, a.part.clone()));
        }
        if !self.format || !self.adopt.is_empty() {
            return Ok((adopts, None));
        }
        let disks: Vec<&str> = self
            .format_disks
            .iter()
            .filter(|d| d.2)
            .map(|d| d.0.as_str())
            .collect();
        ensure!(!disks.is_empty(), "Keine Platte gewählt");
        ensure!(
            self.format_confirm.trim() == "LÖSCHEN",
            "Zum Bestätigen LÖSCHEN eintippen"
        );
        let name = self.format_name.trim().to_ascii_lowercase();
        ensure!(
            valid_volume(&name),
            "Laufwerksname: a–z, 0–9, - und _, höchstens 16 Zeichen"
        );
        let args = if disks.len() == 2 && self.format_mirror {
            json!({"layout": "mirror", "name": name, "disks": disks, "fs": "ext4", "confirm": true})
        } else {
            let volumes: Vec<Value> = disks
                .iter()
                .enumerate()
                .map(|(i, d)| json!({"name": if i == 0 { name.clone() } else { format!("{name}{}", i + 1) }, "disk": d, "fs": "ext4"}))
                .collect();
            json!({"layout": "single", "volumes": volumes, "confirm": true})
        };
        Ok((adopts, Some(args)))
    }

    fn user_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(egui::RichText::new("3 · Erster Benutzer").strong());
        let volume = self.storage["volumes"][0]["name"]
            .as_str()
            .map(String::from);
        small(ui, "Für den Alltag einen eigenen Benutzer anlegen (root nur für die Verwaltung). Leer lassen zum Überspringen.");
        crate::field(ui, "Benutzername", &mut self.user);
        ui.horizontal(|ui| {
            ui.label("Passwort");
            ui.add(
                crate::text_input(&mut self.user_password)
                    .password(true)
                    .desired_width(200.0),
            );
            ui.label("wiederholen");
            ui.add(
                crate::text_input(&mut self.user_password2)
                    .password(true)
                    .desired_width(200.0),
            );
        });
        if let Some(v) = &volume {
            ui.checkbox(&mut self.share, format!("Freigabe „{v}“ (das ganze Laufwerk) mit Schreibrecht für diesen Benutzer anlegen"));
        }
        ui.horizontal(|ui| {
            if ui.button("Fertigstellen").clicked() {
                let user = self.user.trim().to_string();
                if user.is_empty() {
                    self.stage = Stage::Done;
                    return;
                }
                if let Err(e) = self.user_check(&user) {
                    self.error = e.to_string();
                    return;
                }
                let password = std::mem::take(&mut self.user_password);
                self.user_password2.clear();
                let share = self.share.then(|| volume.clone()).flatten();
                let nas = self.nas.clone();
                let home = volume.clone().unwrap_or_default();
                self.run(ctx, move |_| {
                    let nas = nas.context("nicht angemeldet")?;
                    let mut done = vec![];
                    nas.rpc(
                        "shareos.accounts",
                        "add",
                        json!({"name": user, "role": "user", "password": password, "home": home, "web": true, "files": true}),
                    )?;
                    done.push(format!("Benutzer „{user}“ angelegt{}", if home.is_empty() { "" } else { " (persönlicher Ordner)" }));
                    if let Some(v) = share {
                        nas.rpc(
                            "shareos.shares",
                            "set",
                            json!({"name": v, "volume": v, "path": "", "comment": "", "smb": true, "rw": [user]}),
                        )?;
                        done.push(format!("Freigabe „{v}“ für {user}"));
                    }
                    Ok(Step::UserDone(done))
                });
            }
        });
    }
    fn user_check(&self, user: &str) -> Result<()> {
        ensure!(
            valid_user(user),
            "Benutzername: a–z, 0–9 und _, mit einem Buchstaben beginnen, höchstens 32 Zeichen"
        );
        ensure!(user != "root", "root gibt es schon");
        ensure!(
            self.user_password == self.user_password2,
            "Die Passwörter stimmen nicht überein"
        );
        ensure!(
            self.user_password.chars().count() >= 6,
            "Passwort zu kurz (mindestens 6 Zeichen)"
        );
        Ok(())
    }

    fn confirm_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, p: &Palette) {
        let (Some(fw), Some(checked)) = (&self.firmware, &self.checked) else {
            return;
        };
        match checked {
            altf::Checked::Refused(msg) => {
                warn(
                    ui,
                    p.error,
                    "Alt-F lehnt die Firmware ab – es wurde nichts geflasht:",
                );
                small(ui, msg.as_str());
                if ui.button("Abbrechen").clicked() {
                    if let Some(alt) = self.alt.clone() {
                        self.run(ctx, move |_| {
                            alt.flash("Abort")?;
                            Ok(Step::AltLoggedIn(Box::new(alt)))
                        });
                    }
                }
                return;
            }
            altf::Checked::Ready(msg) | altf::Checked::NeedsSpecialReboot(msg) => {
                ui.label(
                    egui::RichText::new("Alt-F hat die Firmware geprüft und akzeptiert.").strong(),
                );
                small(ui, msg.as_str());
            }
        }
        ui.add_space(6.0);
        text(ui, format!("Box: {}", self.address));
        text(
            ui,
            format!("Firmware: ShareOS {} für {}", fw.version, fw.model),
        );
        small(ui, format!("SHA-256: {}", fw.sha256));
        if matches!(checked, altf::Checked::NeedsSpecialReboot(_)) {
            small(ui, "Alt-F läuft aus dem Flash: die App macht zuerst einen Spezial-Neustart und lädt die Firmware danach noch einmal hoch.");
        }
        ui.add_space(6.0);
        warn(ui, p.error, "Jetzt werden Kernel und Startsystem im Flash ersetzt. Daten auf den Platten, U-Boot und das D-Link-Rettungssystem bleiben unberührt.");
        small(ui, "Während des Flashens den Strom NICHT trennen. Danach installiert ShareOS sein System selbst – von einem USB-Stick mit der passenden .sysupgrade.tar, sonst aus dem Internet (Netzwerkkabel und DHCP nötig). Das dauert einige Minuten. Die Einstellungen von Alt-F im Flash bleiben erhalten; ShareOS übernimmt daraus Benutzer und Freigaben.");
        ui.checkbox(
            &mut self.understood,
            "Verstanden – die Box auf ShareOS umstellen",
        );
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.understood, egui::Button::new("Jetzt umstellen"))
                .clicked()
            {
                self.switch(ctx);
            }
            if ui.button("Abbrechen").clicked() {
                if let Some(alt) = self.alt.clone() {
                    self.firmware = None;
                    self.checked = None;
                    self.run(ctx, move |_| {
                        alt.flash("Abort")?;
                        Ok(Step::AltLoggedIn(Box::new(alt)))
                    });
                }
            }
        });
    }
}

fn logged_in(nas: Nas) -> Result<Step> {
    ensure!(nas.admin, "Für die Einrichtung ist ein Administrator nötig");
    let info = nas.rpc("shareos.setup", "status", json!({}))?;
    let zones: Vec<String> = nas.rpc("shareos.setup", "zones", json!({}))?["zones"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|z| z.as_str().map(String::from))
        .collect();
    let altf = nas
        .rpc("shareos.setup", "altf", json!({}))
        .unwrap_or(Value::Null);
    Ok(Step::LoggedIn(Box::new(nas), info, zones, altf))
}

/// partitions (and RAID arrays) with a file system that no volume uses yet
fn collect_parts(node: &Value, used: &[String], out: &mut Vec<Adopt>) {
    for c in node["children"].as_array().into_iter().flatten() {
        let name = c["name"].as_str().unwrap_or("");
        let fs = c["fstype"].as_str().unwrap_or("");
        let kind = c["type"].as_str().unwrap_or("");
        if (kind == "part" || kind.starts_with("raid"))
            && ADOPTABLE.contains(&fs)
            && !used.iter().any(|u| u == name)
            && !out.iter().any(|a| a.part == name)
            && c["size"].as_u64().unwrap_or(0) > 1 << 30
        {
            out.push(Adopt {
                part: name.into(),
                fs: fs.into(),
                size: c["size"].as_u64().unwrap_or(0),
                take: true,
                name: String::new(),
            });
        }
        collect_parts(c, used, out);
    }
}

fn text(ui: &mut egui::Ui, s: impl Into<egui::WidgetText>) {
    ui.add(egui::Label::new(s).wrap());
}
fn small(ui: &mut egui::Ui, s: impl Into<String>) {
    ui.add(egui::Label::new(egui::RichText::new(s).small()).wrap());
}
fn warn(ui: &mut egui::Ui, color: egui::Color32, s: impl Into<String>) {
    ui.add(egui::Label::new(egui::RichText::new(s).color(color)).wrap());
}

/// try `f` every 10 seconds until it works or the time is up
fn wait_for<T>(limit: Duration, mut f: impl FnMut() -> Result<T>) -> Result<T> {
    let start = Instant::now();
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) if start.elapsed() >= limit => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_secs(10)),
        }
    }
}

fn valid_hostname(h: &str) -> bool {
    let b = h.as_bytes();
    !b.is_empty()
        && b.len() <= 63
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-')
        && b[0] != b'-'
        && b[b.len() - 1] != b'-'
}
fn valid_volume(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 16
        && n.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
}
fn valid_user(n: &str) -> bool {
    let b = n.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_are_checked_like_the_box() {
        assert!(
            valid_hostname("shareos")
                && valid_hostname("nas-1")
                && !valid_hostname("-x")
                && !valid_hostname("a b")
        );
        assert!(
            valid_volume("daten")
                && !valid_volume("Daten")
                && !valid_volume("viel-zu-langer-name-x")
        );
        assert!(valid_user("anna") && !valid_user("1anna") && !valid_user("Anna"));
    }
    #[test]
    fn existing_file_systems_are_offered_for_taking_over_not_formatting() {
        let st = json!({
            "disks": [
                {"name":"sda","size": 2_000_000_000_000u64,"usb":false,"children":[
                    {"name":"sda1","type":"part","fstype":"swap","size": 536_870_912u64},
                    {"name":"sda2","type":"part","fstype":"ext4","size": 1_990_000_000_000u64}]},
                {"name":"sdb","size": 2_000_000_000_000u64,"usb":false,"children":[]},
                {"name":"sdc","size": 32_000_000_000u64,"usb":true,"children":[{"name":"sdc1","type":"part","fstype":"vfat","size": 32_000_000_000u64}]}
            ],
            "volumes": []
        });
        let mut s = Setup::new("http://box");
        s.plan_disks(&st);
        assert_eq!(s.adopt.len(), 1);
        assert_eq!(
            (s.adopt[0].part.as_str(), s.adopt[0].name.as_str()),
            ("sda2", "daten")
        );
        assert_eq!(s.format_disks.len(), 2, "USB drives are not offered");
        // with a partition to take over, nothing is formatted
        s.format = true;
        let (adopts, format) = s.disk_jobs().unwrap();
        assert_eq!(adopts, vec![("daten".to_string(), "sda2".to_string())]);
        assert!(format.is_none());
        // a used partition is not offered again
        let mut st2 = st.clone();
        st2["volumes"] = json!([{"name":"daten","device":"/dev/sda2"}]);
        let mut s2 = Setup::new("http://box");
        s2.plan_disks(&st2);
        assert!(s2.adopt.is_empty());
    }
    #[test]
    fn formatting_needs_the_typed_confirmation() {
        let st = json!({"disks":[
            {"name":"sda","size":1u64<<40,"usb":false,"children":[]},
            {"name":"sdb","size":1u64<<40,"usb":false,"children":[]}],"volumes":[]});
        let mut s = Setup::new("http://box");
        s.plan_disks(&st);
        s.format = true;
        assert!(s.format_mirror);
        assert!(s.disk_jobs().is_err());
        s.format_confirm = "LÖSCHEN".into();
        let (_, args) = s.disk_jobs().unwrap();
        let args = args.unwrap();
        assert_eq!(args["layout"], "mirror");
        assert_eq!(args["confirm"], true);
        assert_eq!(args["disks"], json!(["sda", "sdb"]));
        s.format_mirror = false;
        let (_, args) = s.disk_jobs().unwrap();
        assert_eq!(args.unwrap()["volumes"][1]["name"], "daten2");
        s.format = false;
        assert!(s.disk_jobs().unwrap().1.is_none());
    }
    #[test]
    fn basics_are_validated_before_sending() {
        let mut s = Setup::new("http://box");
        s.hostname = "nas".into();
        s.password = "geheim1".into();
        s.password2 = "geheim2".into();
        assert!(s.basics_args().is_err());
        s.password2 = "geheim1".into();
        let a = s.basics_args().unwrap();
        assert_eq!(a["hostname"], "nas");
        assert_eq!(a["altf"], false);
        s.password = "kurz".into();
        s.password2 = "kurz".into();
        assert!(s.basics_args().is_err());
        // the factory password must be replaced
        s.default_password = true;
        s.password.clear();
        s.password2.clear();
        assert!(s.basics_args().is_err());
        s.password = "shareos".into();
        s.password2 = "shareos".into();
        assert!(s.basics_args().is_err());
        s.password = "geheim1".into();
        s.password2 = "geheim1".into();
        assert!(s.basics_args().is_ok());
    }
    #[test]
    fn opened_from_the_search_checks_the_box() {
        assert!(Setup::detected("http://box").auto_detect);
        assert!(!Setup::new("http://box").auto_detect);
    }
}
