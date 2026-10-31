#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod backup;
mod browser;
mod drives;
mod nas;
mod theme;
mod tls;
mod transfer;
use anyhow::Result;
use eframe::egui;
use nas::{Entry, Nas, Root};
use serde_json::Value;
use std::{
    io::Write,
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver},
        Arc,
    },
};

const TABS: [&str; 6] = [
    "Box finden",
    "Dateien",
    "Netzlaufwerke",
    "Dateibackup",
    "Box-Einstellungen",
    "Terminal",
];
enum Change {
    Message(String),
    Found(Vec<String>),
    Certificate(tls::Certificate),
    Login(Nas, Vec<Root>),
    List(Vec<Entry>, bool),
    Snapshot(backup::Snapshot),
    Preview(Value, String, String),
    Logout,
    Reload(String),
}
struct App {
    tab: usize,
    address: String,
    subnet: String,
    user: String,
    password: String,
    certificate: Option<tls::Certificate>,
    trusted_certificate: Option<tls::Certificate>,
    nas: Option<Nas>,
    roots: Vec<Root>,
    selected: Vec<bool>,
    root: String,
    path: String,
    entries: Vec<Entry>,
    found: Vec<String>,
    local: String,
    file_filter: String,
    show_hidden: bool,
    file_sort: browser::Sort,
    descending: bool,
    new_folder: String,
    transfer: Option<Arc<transfer::Progress>>,
    backup_dir: String,
    snapshot_dir: String,
    snapshot: Option<backup::Snapshot>,
    loaded_snapshot_dir: String,
    share: String,
    drive: String,
    settings_file: String,
    settings_password: String,
    preview: Option<(Value, String, String)>,
    confirm: bool,
    status: String,
    job: Option<Receiver<Result<Change>>>,
    logo_light: egui::TextureHandle,
    logo_dark: egui::TextureHandle,
    dark: bool,
}
impl App {
    fn new(ctx: &egui::Context) -> Self {
        theme::apply(ctx, true);
        let load_logo = |name, bytes: &[u8]| {
            let image = image::load_from_memory(bytes).unwrap().to_rgba8();
            let size = [image.width() as usize, image.height() as usize];
            ctx.load_texture(
                name,
                egui::ColorImage::from_rgba_unmultiplied(size, &image),
                Default::default(),
            )
        };
        let logo_light = load_logo("logo-light", include_bytes!("../assets/logo-light.png"));
        let logo_dark = load_logo("logo-dark", include_bytes!("../assets/logo-dark.png"));
        Self {
            tab: 0,
            address: "http://192.168.0.32".into(),
            subnet: "192.168.0".into(),
            user: String::new(),
            password: String::new(),
            certificate: None,
            trusted_certificate: None,
            nas: None,
            roots: vec![],
            selected: vec![],
            root: String::new(),
            path: String::new(),
            entries: vec![],
            found: vec![],
            local: String::new(),
            file_filter: String::new(),
            show_hidden: false,
            file_sort: browser::Sort::default(),
            descending: false,
            new_folder: String::new(),
            transfer: None,
            backup_dir: String::new(),
            snapshot_dir: String::new(),
            snapshot: None,
            loaded_snapshot_dir: String::new(),
            share: String::new(),
            drive: "S:".into(),
            settings_file: String::new(),
            settings_password: String::new(),
            preview: None,
            confirm: false,
            status: "Box suchen oder Adresse eingeben und anmelden.".into(),
            job: None,
            logo_light,
            logo_dark,
            dark: true,
        }
    }
    fn start(
        &mut self,
        ctx: &egui::Context,
        job: impl FnOnce() -> Result<Change> + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        self.transfer = None;
        self.job = Some(rx);
        self.status = "Wird ausgeführt …".into();
        std::thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint();
        });
    }
    fn refresh(&mut self, ctx: &egui::Context) {
        if let Some(nas) = self.nas.clone() {
            let root = self.root.clone();
            let path = self.path.clone();
            self.entries.clear();
            self.start(ctx, move || {
                let list = nas.list(&root, &path)?;
                Ok(Change::List(list.entries, list.truncated))
            });
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.job else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.job = None;
                self.transfer = None;
                match result {
                    Err(error) => self.status = format!("Fehler: {error:#}"),
                    Ok(change) => {
                        self.status = "Fertig.".into();
                        match change {
                            Change::Message(message) => self.status = message,
                            Change::Reload(message) => {
                                self.refresh(ctx);
                                self.status = message;
                            }
                            Change::Found(found) => {
                                self.status = if found.is_empty() {
                                    "Keine Box gefunden. Subnetz und Box-Adresse prüfen; auch die eingegebene Adresse wird durchsucht.".into()
                                } else {
                                    format!("{} Adresse(n) gefunden. HTTPS-Zertifikat vor der Anmeldung prüfen.", found.len())
                                };
                                self.found = found;
                            }
                            Change::Certificate(certificate) => {
                                self.certificate = Some(certificate);
                                self.status = "Zertifikat geladen. Fingerabdruck vor dem Vertrauen mit der Box vergleichen.".into();
                            }
                            Change::Login(nas, roots) => {
                                self.selected = vec![!nas.admin; roots.len()];
                                self.root =
                                    roots.first().map(|x| x.name.clone()).unwrap_or_default();
                                self.nas = Some(nas);
                                self.roots = roots;
                                self.path.clear();
                                self.entries.clear();
                                self.file_filter.clear();
                                self.tab = 1;
                                if !self.root.is_empty() {
                                    self.refresh(ctx);
                                }
                                self.status = "Angemeldet. Dateien werden geladen.".into();
                            }
                            Change::List(entries, truncated) => {
                                self.entries = entries;
                                if truncated {
                                    self.status =
                                        "Die NAS-API zeigt nur die ersten 2000 Einträge.".into();
                                }
                            }
                            Change::Snapshot(snapshot) => {
                                self.snapshot = Some(snapshot);
                                self.loaded_snapshot_dir = self.snapshot_dir.clone();
                            }
                            Change::Preview(value, content, password) => {
                                self.preview = Some((value, content, password));
                                self.confirm = false;
                            }
                            Change::Logout => {
                                self.nas = None;
                                self.trusted_certificate = None;
                                self.certificate = None;
                                self.roots.clear();
                                self.entries.clear();
                                self.selected.clear();
                                self.preview = None;
                            }
                        }
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(_) => {
                self.job = None;
                self.status = "Hintergrundaufgabe wurde unterbrochen.".into();
            }
        }
    }
    fn discovery(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label("Box im lokalen Netzwerk suchen");
        field(ui, "Subnetz (optional)", &mut self.subnet);
        if ui.button("Boxen suchen").clicked() {
            let subnet = self.subnet.clone();
            let configured = self.address.clone();
            self.start(ctx, move || {
                Ok(Change::Found(nas::discover(&subnet, &configured)?))
            });
        }
        for address in &self.found {
            if ui
                .selectable_label(self.address == *address, address)
                .clicked()
            {
                self.address = address.clone();
            }
        }
        ui.separator();
        field(ui, "Box-Adresse", &mut self.address);
        ui.small("HTTPS verwenden. Bei HTTP werden Zugangsdaten unverschlüsselt übertragen.");
        let origin = nas::base_url(&self.address)
            .ok()
            .map(|url| url.origin().ascii_serialization());
        if self
            .trusted_certificate
            .as_ref()
            .is_some_and(|cert| Some(&cert.origin) != origin.as_ref())
        {
            self.trusted_certificate = None;
        }
        if self
            .certificate
            .as_ref()
            .is_some_and(|cert| Some(&cert.origin) != origin.as_ref())
        {
            self.certificate = None;
        }
        if self.nas.is_none()
            && (self.address.trim().starts_with("https://") || !self.address.contains("://"))
        {
            if ui.button("HTTPS-Zertifikat prüfen").clicked() {
                let address = self.address.clone();
                self.trusted_certificate = None;
                self.certificate = None;
                self.start(ctx, move || {
                    Ok(Change::Certificate(tls::inspect(&address)?))
                });
            }
            if let Some(cert) = &self.certificate {
                ui.label(format!("Zertifikat für {}", cert.origin));
                ui.label("SHA-256-Fingerabdruck:");
                ui.add(
                    egui::Label::new(egui::RichText::new(cert.fingerprint()).monospace()).wrap(),
                );
                ui.small("Mit dem Zertifikat im Browser oder auf der Box vergleichen. Vertrauen gilt nur für diese Adresse und Sitzung, auch bei abweichendem Zertifikatsnamen oder Gültigkeitszeitraum.");
                if self.trusted_certificate.is_none()
                    && ui
                        .button("Diesem Zertifikat für diese Sitzung vertrauen")
                        .clicked()
                {
                    self.trusted_certificate = Some(cert.clone());
                }
            }
            if self.trusted_certificate.is_some() {
                ui.label("Das bestätigte Zertifikat wird für diese Sitzung exakt geprüft.");
                if ui.button("Zertifikatsvertrauen zurücksetzen").clicked() {
                    self.trusted_certificate = None;
                }
            }
        }
        field(ui, "Benutzername", &mut self.user);
        ui.horizontal(|ui| {
            ui.label("Passwort");
            ui.add(egui::TextEdit::singleline(&mut self.password).password(true));
        });
        if self.nas.is_none() && ui.button("Anmelden").clicked() {
            let address = self.address.clone();
            let user = self.user.clone();
            let password = std::mem::take(&mut self.password);
            let trusted = self.trusted_certificate.clone();
            self.start(ctx, move || {
                let nas = Nas::connect(&address, user, &password, trusted.as_ref())?;
                let roots = match nas.roots() {
                    Ok(roots) => roots,
                    Err(error) => {
                        let _ = nas.logout();
                        return Err(error);
                    }
                };
                Ok(Change::Login(nas, roots))
            });
        }
        if self.nas.is_some() {
            ui.label("Zum Wechseln der Box zuerst abmelden.");
        }
    }
    fn files(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(nas) = self.nas.clone() else {
            ui.label("Melde dich an, um deine Dateien und freigegebenen Ordner zu sehen.");
            return;
        };
        if self.roots.is_empty() {
            ui.label("Für dieses Konto sind keine Ordner verfügbar.");
            return;
        }
        let p = theme::palette(self.dark);
        let writable = self
            .roots
            .iter()
            .any(|root| root.name == self.root && !root.ro);
        let mut navigate = None;
        let mut download = None;
        let mut upload = false;
        let mut mkdir = false;
        ui.horizontal_top(|ui| {
            egui::Frame::new().fill(p.bg).corner_radius(10).inner_margin(12).show(ui, |ui| {
                ui.set_width(175.0);
                ui.label(egui::RichText::new("MEINE BOX").small().strong().color(p.muted));
                ui.add_space(4.0);
                for root in &self.roots {
                    let title = if root.home { "Eigene Dateien" } else { &root.name };
                    let selected = self.root == root.name;
                    let response = ui.add_sized([175.0,40.0], egui::Button::new(egui::RichText::new(title).color(if selected {p.bg} else {p.text}))
                        .fill(if selected {p.accent} else {p.bg}));
                    if response.on_hover_text(if root.ro {"Nur lesen"} else {"Lesen und schreiben"}).clicked() {
                        navigate = Some((root.name.clone(),String::new()));
                    }
                    if root.ro { ui.label(egui::RichText::new("nur lesen").small().color(p.muted)); }
                }
            });
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    if ui.button(&self.root).on_hover_text("Zum Anfang dieses Ordners").clicked() {
                        navigate = Some((self.root.clone(),String::new()));
                    }
                    let parts: Vec<_> = self.path.split('/').filter(|part| !part.is_empty()).collect();
                    for (i,part) in parts.iter().enumerate() {
                        ui.label(egui::RichText::new("/").color(p.muted));
                        if ui.link(*part).clicked() { navigate=Some((self.root.clone(),parts[..=i].join("/"))); }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!self.path.is_empty(),egui::Button::new("Nach oben")).clicked() {
                        navigate=Some((self.root.clone(),self.path.rsplit_once('/').map(|(parent,_)|parent.to_string()).unwrap_or_default()));
                    }
                    if ui.button("Aktualisieren").on_hover_text("F5").clicked() { navigate=Some((self.root.clone(),self.path.clone())); }
                    upload=ui.add_enabled(writable,egui::Button::new("Hochladen …")).clicked();
                    ui.label(egui::RichText::new(if writable {"Lesen & Schreiben"} else {"Nur lesen"}).small().color(p.muted));
                });
                ui.add(egui::TextEdit::singleline(&mut self.file_filter).hint_text("In diesem Ordner suchen …").desired_width(f32::INFINITY));
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.show_hidden,"Versteckte Dateien");
                    egui::ComboBox::from_id_salt("file-sort").selected_text(match self.file_sort {browser::Sort::Name=>"Name",browser::Sort::Size=>"Größe",browser::Sort::Modified=>"Geändert"}).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.file_sort,browser::Sort::Name,"Name");
                        ui.selectable_value(&mut self.file_sort,browser::Sort::Size,"Größe");
                        ui.selectable_value(&mut self.file_sort,browser::Sort::Modified,"Geändert");
                    });
                    ui.checkbox(&mut self.descending,"Absteigend");
                });
                let rows = browser::visible(&self.entries,&self.file_filter,self.show_hidden,self.file_sort,self.descending);
                let total_bytes:u64=rows.iter().filter(|e|e.kind=="file").fold(0u64,|total,entry|total.saturating_add(entry.size));
                ui.label(egui::RichText::new(format!("{} Einträge · {}",rows.len(),browser::size(total_bytes))).small().color(p.muted));
                ui.separator();
                if rows.is_empty() {
                    ui.add_space(24.0);
                    ui.label(egui::RichText::new(if self.file_filter.is_empty() {"Dieser Ordner ist leer."} else {"Keine passenden Dateien gefunden."}).color(p.muted));
                    ui.add_space(24.0);
                } else {
                    egui::ScrollArea::both().id_salt("file-list").max_height(420.0).min_scrolled_height(120.0).show(ui, |ui| {
                        egui::Grid::new("files-table").striped(true).spacing([16.0,8.0]).show(ui, |ui| {
                            for title in ["","NAME","GRÖSSE","TYP","GEÄNDERT",""] { ui.label(egui::RichText::new(title).small().strong().color(p.muted)); } ui.end_row();
                            for entry in rows {
                                browser::icon(ui,entry.kind=="dir",if entry.kind=="dir" {p.accent} else {p.muted});
                                let response = ui.add(egui::Button::new(egui::RichText::new(&entry.name).color(p.text)).frame(false));
                                if response.on_hover_text(&entry.name).clicked() && entry.kind=="dir" {
                                    navigate=Some((self.root.clone(),if self.path.is_empty() {entry.name.clone()} else {format!("{}/{}",self.path,entry.name)}));
                                }
                                ui.label(if entry.kind=="file" {browser::size(entry.size)} else {"–".into()});
                                ui.label(egui::RichText::new(browser::kind(entry)).small().color(p.muted));
                                ui.label(egui::RichText::new(browser::modified(entry.mtime)).small().color(p.muted));
                                if entry.kind=="file" {
                                    if ui.button("Speichern …").on_hover_text("Datei auf diesem Rechner speichern").clicked() {download=Some(entry.name.clone());}
                                } else {ui.label("");}
                                ui.end_row();
                            }
                        });
                    });
                }
                ui.separator();
                if writable {
                    ui.collapsing("Neuer Ordner", |ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.new_folder).hint_text("Ordnername").desired_width(240.0));
                            mkdir=ui.add_enabled(!self.new_folder.trim().is_empty(),egui::Button::new("Anlegen")).clicked();
                        });
                    });
                }
                ui.small("Dateien werden mit den Rechten deiner Anmeldung übertragen. Bestehende Dateien bleiben erhalten.");
            });
        });
        if let Some((root, path)) = navigate {
            self.root = root;
            self.path = path;
            self.file_filter.clear();
            self.refresh(ctx);
            return;
        }
        if mkdir {
            let root = self.root.clone();
            let path = self.path.clone();
            let name = std::mem::take(&mut self.new_folder).trim().to_owned();
            self.start(ctx, move || {
                nas.mkdir(&root, &path, &name)?;
                Ok(Change::Reload(format!("Ordner {name} angelegt.")))
            });
            return;
        }
        if let Some(name) = download {
            let root = self.root.clone();
            let path = self.path.clone();
            let local = self.local.clone();
            let progress = Arc::new(transfer::Progress::default());
            let worker = progress.clone();
            let label = format!("Herunterladen: {name}");
            self.start(ctx, move || {
                anyhow::ensure!(backup::safe_name(&name), "Ungültiger lokaler Dateiname");
                let mut dialog = rfd::FileDialog::new()
                    .set_file_name(&name)
                    .set_title("Datei von der Box speichern");
                if !local.is_empty() {
                    dialog = dialog.set_directory(local);
                }
                let Some(target) = dialog.save_file() else {
                    return Ok(Change::Message("Download abgebrochen.".into()));
                };
                nas.download_progress(&root, &path, &name, &target, &worker)?;
                Ok(Change::Message(format!(
                    "Gespeichert: {}",
                    target.display()
                )))
            });
            self.transfer = Some(progress);
            self.status = label;
            return;
        }
        if upload {
            let root = self.root.clone();
            let path = self.path.clone();
            let progress = Arc::new(transfer::Progress::default());
            let worker = progress.clone();
            self.start(ctx, move || {
                let Some(source) = rfd::FileDialog::new()
                    .set_title("Datei auf die Box hochladen")
                    .pick_file()
                else {
                    return Ok(Change::Message("Upload abgebrochen.".into()));
                };
                nas.upload_progress(&root, &path, &source, worker)?;
                Ok(Change::Reload(format!(
                    "{} hochgeladen.",
                    source.file_name().unwrap_or_default().to_string_lossy()
                )))
            });
            self.transfer = Some(progress);
            self.status = "Datei auswählen und hochladen …".into();
        }
    }
    fn terminal(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(nas) = self.nas.clone().filter(|nas| nas.admin) else {
            ui.label("Das Terminal steht ausschließlich Administratoren zur Verfügung.");
            return;
        };
        let p = theme::palette(self.dark);
        ui.label(
            egui::RichText::new("root@shareos")
                .monospace()
                .size(22.0)
                .color(p.accent),
        );
        ui.label(
            "Eine interaktive Shell auf deiner Box – über das vorhandene ShareOS-Web-Terminal.",
        );
        ui.label("Die Sitzung öffnet sich im Standardbrowser. Das Schließen des Terminal-Tabs beendet die Sitzung.");
        ui.small("Bei HTTPS prüft der Browser das Box-Zertifikat separat. Eine neue Terminal-Sitzung ersetzt eine bereits laufende.");
        if ui.button("Terminal im Browser öffnen").clicked() {
            let dark = self.dark;
            self.start(ctx,move || {
                let url=nas.terminal(dark)?;
                webbrowser::open(url.as_str()).map_err(|_|anyhow::anyhow!("Terminal gestartet, aber der Browser konnte nicht geöffnet werden. Standardbrowser prüfen und erneut versuchen."))?;
                Ok(Change::Message("Admin-Terminal im Browser geöffnet.".into()))
            });
        }
        ui.separator();
        ui.label(
            egui::RichText::new("Befehle laufen als root und wirken sofort auf die Box.")
                .color(p.error),
        );
        ui.collapsing("Nützliche Diagnosebefehle", |ui| {
            for (command, description) in [
                ("uptime", "Laufzeit und Systemlast"),
                ("df -h", "Freier Speicherplatz"),
                ("free -m", "Arbeitsspeicher"),
                ("logread | tail -n 50", "Letzte Systemmeldungen"),
                ("ip addr", "Netzwerkadressen"),
            ] {
                ui.horizontal(|ui| {
                    ui.monospace(command);
                    ui.label(description);
                    if ui.small_button("Kopieren").clicked() {
                        ui.ctx().copy_text(command.into());
                    }
                });
            }
        });
    }
    fn network(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label("SMB-Netzlaufwerk über das Betriebssystem verbinden");
        field(ui, "Box-Adresse", &mut self.address);
        field(ui, "SMB-Freigabename", &mut self.share);
        #[cfg(target_os = "windows")]
        field(ui, "Laufwerksbuchstabe", &mut self.drive);
        ui.label("Die SMB-Anmeldung erfolgt separat durch das Betriebssystem. Sie verwendet keine Desktop-Sitzung.");
        ui.small("Windows: gespeicherte Windows-Anmeldeinformationen. macOS: Finder-Anmeldung. Linux: GIO/GVfs mit SMB-Unterstützung erforderlich.");
        if ui.button("Netzlaufwerk verbinden").clicked() {
            let address = self.address.clone();
            let share = self.share.clone();
            let drive = self.drive.clone();
            self.start(ctx, move || {
                Ok(Change::Message(drives::connect(&address, &share, &drive)?))
            });
        }
    }
    fn backups(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label("Lokale Sicherungsstände");
        if let Some(nas) = &self.nas {
            ui.label("Eigene Dateien und freigegebene Ordner mit den Rechten der aktuellen Anmeldung auswählen.");
            if nas.admin {
                ui.colored_label(egui::Color32::YELLOW, "Administrator: Die NAS-API bietet ganze Laufwerke an. Auswahl vor dem Sichern prüfen.");
            }
            for (i, root) in self.roots.iter().enumerate() {
                ui.checkbox(
                    &mut self.selected[i],
                    format!(
                        "{}{}",
                        root.name,
                        if root.home { " · Eigene Dateien" } else { "" }
                    ),
                );
            }
            field(ui, "Backup-Zielordner", &mut self.backup_dir);
            if ui.button("Jetzt sichern").clicked() {
                let nas = nas.clone();
                let roots = self
                    .roots
                    .iter()
                    .zip(&self.selected)
                    .filter(|(_, selected)| **selected)
                    .map(|(root, _)| root.clone())
                    .collect();
                let dest = self.backup_dir.clone();
                self.start(ctx, move || {
                    anyhow::ensure!(!dest.trim().is_empty(), "Backup-Zielordner angeben");
                    Ok(Change::Message(backup::create(
                        &nas,
                        roots,
                        &PathBuf::from(dest),
                    )?))
                });
            }
        } else {
            ui.label("Zum Erstellen einer Sicherung bitte anmelden.");
        }
        ui.small("Links und Spezialdateien werden ausgelassen. Abgeschnittene NAS-Verzeichnislisten brechen die Sicherung ab. Dateien werden nacheinander gelesen; kein atomarer NAS-Snapshot.");
        ui.separator();
        field(
            ui,
            "Sicherungsstand (snapshot-…-Ordner)",
            &mut self.snapshot_dir,
        );
        if ui.button("Sicherungsstand öffnen").clicked() {
            let dir = self.snapshot_dir.clone();
            self.snapshot = None;
            self.start(ctx, move || {
                Ok(Change::Snapshot(backup::load(&PathBuf::from(dir))?))
            });
        }
        field(ui, "Wiederherstellungs-Zielordner", &mut self.local);
        let mut restore = None;
        if let Some(snapshot) = &self.snapshot {
            ui.label(format!(
                "{} · {} · {} Dateien",
                snapshot.box_url,
                snapshot.user,
                snapshot.files.len()
            ));
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    for (i, file) in snapshot.files.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}/{}/{}", file.root, file.path, file.name));
                            if ui.button("Wiederherstellen").clicked() {
                                restore = Some(i);
                            }
                        });
                    }
                });
        }
        if let Some(i) = restore {
            let dir = self.loaded_snapshot_dir.clone();
            let dest = self.local.clone();
            self.start(ctx, move || {
                anyhow::ensure!(!dest.trim().is_empty(), "Zielordner angeben");
                Ok(Change::Message(backup::restore(
                    &PathBuf::from(dir),
                    i,
                    &PathBuf::from(dest),
                )?))
            });
        }
        ui.small("Einzelne Dateien werden lokal mit SHA-256-Prüfung wiederhergestellt. Vorhandene Dateien bleiben erhalten. Upload zurück zur Box ist unter Dateien möglich.");
    }
    fn settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(nas) = self.nas.clone().filter(|n| n.admin) else {
            ui.label("Für Box-Einstellungen mit einem Administratorkonto anmelden.");
            return;
        };
        field(ui, "Lokale Sicherungsdatei (JSON)", &mut self.settings_file);
        ui.horizontal(|ui| {
            ui.label("Sicherungskennwort");
            ui.add(egui::TextEdit::singleline(&mut self.settings_password).password(true));
        });
        ui.small("Das Kennwort verschlüsselt die Box-Sicherung. Ohne Kennwort wird sie unverschlüsselt gespeichert.");
        if ui.button("Box-Einstellungen sichern").clicked() {
            let nas = nas.clone();
            let dest = self.settings_file.clone();
            let password = std::mem::take(&mut self.settings_password);
            self.start(ctx, move || {
                let data = nas.settings(&password)?;
                let content = data["content"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Sicherungsinhalt fehlt"))?;
                let mut file = nas::private_file(&PathBuf::from(dest))?;
                file.write_all(content.as_bytes())?;
                file.sync_all()?;
                Ok(Change::Message("Box-Einstellungen gesichert.".into()))
            });
        }
        if ui.button("Wiederherstellung prüfen").clicked() && self.job.is_none() {
            self.preview = None;
            let nas = nas.clone();
            let source = self.settings_file.clone();
            let password = std::mem::take(&mut self.settings_password);
            self.start(ctx, move || {
                let content = std::fs::read_to_string(source)?;
                let preview = nas.restore_settings(&content, &password, false)?;
                Ok(Change::Preview(preview, content, password))
            });
        }
        if let Some((preview, _, _)) = &self.preview {
            ui.separator();
            ui.label(serde_json::to_string_pretty(preview).unwrap_or_default());
            ui.checkbox(
                &mut self.confirm,
                "Aktuelle Einstellungen, Benutzer und Freigaben durch diese Sicherung ersetzen.",
            );
            if ui
                .add_enabled(
                    self.confirm && self.job.is_none(),
                    egui::Button::new("Geprüfte Einstellungen zurückspielen"),
                )
                .clicked()
            {
                let (_, content, password) = self.preview.take().unwrap();
                self.confirm = false;
                self.start(ctx, move || {
                    nas.restore_settings(&content, &password, true)?;
                    let _ = nas.logout();
                    Ok(Change::Logout)
                });
                self.status = "Einstellungen werden zurückgespielt. Danach Box manuell neu starten und erneut anmelden.".into();
            }
        }
    }
}
fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::TextEdit::singleline(value).desired_width(380.0));
    });
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.draw(ctx);
    }
}
impl App {
    fn draw(&mut self, ctx: &egui::Context) {
        self.poll(ctx);
        if self.tab == 5 && !self.nas.as_ref().is_some_and(|nas| nas.admin) {
            self.tab = 0;
        }
        if self.tab == 1
            && self.job.is_none()
            && ctx.input(|input| input.key_pressed(egui::Key::F5))
        {
            self.refresh(ctx);
        }
        let p = theme::palette(self.dark);
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ctx, |ui| {
                ui.set_max_width(1240.0);
                ui.horizontal(|ui| {
                    let logo = if self.dark {
                        &self.logo_dark
                    } else {
                        &self.logo_light
                    };
                    ui.add(egui::Image::new(logo).max_height(72.0).max_width(210.0));
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("DESKTOP")
                                .size(22.0)
                                .strong()
                                .color(p.accent),
                        );
                        ui.label(
                            egui::RichText::new(concat!("Version · ", env!("CARGO_PKG_VERSION")))
                                .size(13.0)
                                .color(p.muted),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let next = if self.dark { "Hell" } else { "Dunkel" };
                        if ui
                            .button(next)
                            .on_hover_text("Hell/Dunkel umschalten")
                            .clicked()
                        {
                            self.dark = !self.dark;
                            theme::apply(ctx, self.dark);
                        }
                    });
                });
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    for (i, tab) in TABS.iter().enumerate() {
                        if i == 5 && !self.nas.as_ref().is_some_and(|nas| nas.admin) {
                            continue;
                        }
                        let active = self.tab == i;
                        let text = egui::RichText::new(tab.to_uppercase())
                            .size(12.0)
                            .strong()
                            .color(if active { p.bg } else { p.muted });
                        let button = egui::Button::new(text)
                            .min_size(egui::vec2(0.0, 44.0))
                            .fill(if active { p.accent } else { p.panel })
                            .stroke(egui::Stroke::new(
                                1.0_f32,
                                if active { p.accent } else { p.line },
                            ));
                        if ui.add(button).clicked() {
                            self.tab = i;
                        }
                    }
                });
                if let Some(nas) = self.nas.clone() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} · {} · {}",
                                nas.user,
                                nas.base,
                                if nas.admin {
                                    "Administrator"
                                } else {
                                    "Benutzer"
                                }
                            ))
                            .color(p.muted),
                        );
                        if ui
                            .add_enabled(self.job.is_none(), egui::Button::new("Abmelden"))
                            .clicked()
                        {
                            self.nas = None;
                            self.trusted_certificate = None;
                            self.certificate = None;
                            self.roots.clear();
                            self.entries.clear();
                            self.preview = None;
                            self.password.clear();
                            self.settings_password.clear();
                            self.start(ctx, move || {
                                let _ = nas.logout();
                                Ok(Change::Logout)
                            });
                        }
                    });
                }
            });
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(24, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if self.job.is_some() {
                        ui.spinner();
                    }
                    let color = if self.status.starts_with("Fehler:") {
                        p.error
                    } else {
                        p.muted
                    };
                    ui.label(egui::RichText::new(&self.status).size(12.0).color(color));
                    if let Some(progress)=&self.transfer {
                        let (done,total)=progress.values();
                        if total>0 {
                            ui.add(egui::ProgressBar::new((done as f64/total as f64).min(1.0) as f32).desired_width(180.0).text(format!("{} / {}",browser::size(done),browser::size(total))));
                        } else {ui.label(browser::size(done));}
                        ctx.request_repaint_after(std::time::Duration::from_millis(100));
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(24, 12)),
            )
            .show(ctx, |ui| {
                ui.set_max_width(1240.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Frame::new()
                        .fill(p.panel)
                        .stroke(egui::Stroke::new(1.0_f32, p.line))
                        .corner_radius(12)
                        .inner_margin(18)
                        .show(ui, |ui| {
                            ui.set_min_width((ui.available_width() - 36.0).max(0.0));
                            ui.label(
                                egui::RichText::new(TABS[self.tab].to_uppercase())
                                    .size(15.0)
                                    .strong()
                                    .color(p.accent),
                            );
                            ui.add_space(8.0);
                            let busy = self.job.is_some();
                            ui.add_enabled_ui(!busy, |ui| match self.tab {
                                0 => self.discovery(ui, ctx),
                                1 => self.files(ui, ctx),
                                2 => self.network(ui, ctx),
                                3 => self.backups(ui, ctx),
                                4 => self.settings(ui, ctx),
                                _ => self.terminal(ui, ctx),
                            });
                        });
                });
            });
    }
}
fn main() -> eframe::Result {
    let image = image::load_from_memory(include_bytes!("../assets/icon-512.png"))
        .unwrap()
        .to_rgba8();
    let icon = egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    };
    eframe::run_native(
        "ShareOS Desktop",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_app_id("de.shareos.desktop")
                .with_inner_size([1060.0, 740.0])
                .with_icon(icon),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))),
    )
}

#[cfg(test)]
mod ui_tests {
    use super::*;
    #[test]
    fn all_views_render_in_both_themes_without_a_display_server() {
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx);
        app.nas = Some(Nas::test_session(true));
        app.roots = vec![Root {
            name: "Team".into(),
            home: false,
            ro: false,
        }];
        app.selected = vec![false];
        app.root = "Team".into();
        app.entries = serde_json::from_value(serde_json::json!([
            {"name":"Dokumente","type":"dir","size":0},
            {"name":"Urlaub.jpg","type":"file","size":123456}
        ]))
        .unwrap();
        for dark in [true, false] {
            app.dark = dark;
            theme::apply(&ctx, dark);
            for tab in 0..TABS.len() {
                app.tab = tab;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1200.0, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.draw(ctx),
                );
                assert!(!output.shapes.is_empty());
                assert!(app.job.is_none());
            }
        }
    }
}
