# ShareOS Desktop

Früher Rust-Prototyp für Windows, Linux und macOS mit egui/eframe. Linux-Build und zwölf automatisierte Tests sind geprüft; die grafische Oberfläche und die plattformspezifischen Integrationen sind noch nicht durchgängig getestet. Eigenständiges Projekt im Repository [UniqueDroid/ShareOS-Desktop](https://github.com/UniqueDroid/ShareOS-Desktop) neben dem Firmware-Repository. Die Firmware bleibt unverändert. Die beiden Original-Logos stammen aus `ShareOS/src/shareos-core/www/` (ShareOS, GPLv2).

## Starten und prüfen

Rust stable mit Cargo installieren, dann im Projektverzeichnis:

```sh
cargo run --locked
cargo test --locked
cargo build --release --locked
```

Die Binärdatei liegt unter `target/release/shareos-desktop` (Windows: `.exe`). Auf jedem Zielsystem nativ bauen. Windows benötigt die MSVC-Buildtools, macOS die Xcode Command Line Tools, Linux einen C-Linker und X11/Wayland/OpenGL-Laufzeitbibliotheken. Linux-SMB benötigt GIO/GVfs mit SMB-Unterstützung. Windows- und macOS-Builds wurden in dieser Linux-Umgebung noch nicht geprüft; es gibt noch keine Installer oder Signierung.

## Die fünf Bereiche

- **Box finden:** `shareos.local`, `192.168.0.32`, die eingegebene Adresse und optional das angegebene IPv4-/24-Subnetz prüfen. Manuelle Box-Adresse mit HTTP oder HTTPS und optional Port möglich. Die Suche identifiziert die bestehende ShareOS-Webanwendung; sie ist keine mDNS-Dienstsuche. HTTPS verwendet standardmäßig den Zertifikatsspeicher des Betriebssystems. Die Suche prüft HTTP und HTTPS, auch bei selbstsignierten Box-Zertifikaten; dabei werden keine Anmeldedaten gesendet. Zusätzlich wird die eingegebene Box-Adresse geprüft. Ein Suchtreffer bedeutet noch kein Zertifikatsvertrauen. Bei HTTP werden auch Anmeldedaten unverschlüsselt übertragen.
- **Dateien:** `session.login`, anschließend verpflichtend `shareos.me.bind`. Ohne erfolgreiche Benutzerzuordnung kein Zugriff und keine angenommene Administratorrolle. Ordner und Leserechte kommen aus `shareos.files.roots`; Listen über `shareos.files.list`. Downloads und Uploads verwenden kurzlebige Einmaltickets von `shareos.files.ticket` und `/cgi-bin/files`. Übertragungen laufen im Hintergrund. Bestehende lokale oder entfernte Dateien werden nicht absichtlich überschrieben. Nach Uploads auf Aktualisieren klicken. Sitzung nach einer Stunde gegebenenfalls erneut anmelden.
- **Netzlaufwerke:** Windows `net.exe use S: \\box\freigabe /persistent:no`, macOS `open smb://…`, Linux `gio mount smb://…`. Die Betriebssystem-Anmeldung ist unabhängig von der Desktop-Websitzung. Unter Windows vorher passende Zugangsdaten in der Anmeldeinformationsverwaltung speichern. Freigabename manuell eingeben; der persönliche SMB-Freigabename entspricht dem Benutzernamen. SMB-Verbindung und Berechtigungen müssen auf dem jeweiligen Zielsystem getestet werden.
- **Dateibackup:** Ausgewählte eigene Ordner und freigegebene Ordner rekursiv lokal sichern, einschließlich versteckter Dateien. Die NAS prüft bei jedem Aufruf die Benutzerrechte. Nur vollständige Durchläufe werden als `snapshot-…` sichtbar; Fehler verwerfen den unvollständigen Sicherungsstand. Bei abgeschnittenen NAS-Verzeichnislisten (mehr als 2000 Einträge) wird abgebrochen. Links und Spezialdateien werden ausgelassen und gezählt. Dateien werden nacheinander gelesen; Änderungen während des Backups sind möglich, es ist kein atomarer NAS-Snapshot. Unterverzeichnisse werden über ihre Dateien gesichert; leere Ordner, POSIX-Rechte und ACLs werden noch nicht wiederhergestellt. Sicherungsdaten bleiben unverschlüsselt auf dem lokalen Datenträger; verschlüsselten Datenträger verwenden, falls erforderlich.
- **Wiederherstellung im Dateibackup:** Sicherungsstand öffnen, lokalen Zielordner angeben und einzelne Datei wiederherstellen. SHA-256 und Dateigröße werden geprüft; bestehende Dateien werden abgewiesen. Dateien landen mit ihrem ursprünglichen Namen im gewählten Zielordner. Für gleichnamige Dateien aus verschiedenen NAS-Ordnern unterschiedliche Zielordner verwenden. Rückübertragung zur Box erfolgt anschließend über Dateien mit aktuellen Schreibrechten.
- **Box-Einstellungen:** Nur bei bestätigter Administratorrolle. `shareos.backup.export` liefert die Sicherungsdatei, optional durch die NAS mit dem angegebenen Kennwort verschlüsselt. Wiederherstellen verwendet zuerst `shareos.backup.restore` mit `apply:false`; erst die explizite Bestätigung spielt genau den geprüften Inhalt mit `apply:true` zurück. Danach Box manuell neu starten und erneut anmelden. Kein automatischer Neustart und kein Firmware-Update.

Für Dateibackups mit dem normalen Benutzer anmelden: Die aktuelle Firmware liefert Administratoren unter `shareos.files.roots` ganze Laufwerke statt nur persönlicher Ordner und Freigaben. Deshalb ist bei Administratorkonten zunächst nichts ausgewählt und die Oberfläche weist auf den größeren Umfang hin.

Keine Passwörter oder Sitzungstokens werden absichtlich auf Platte gespeichert. Einmaltickets werden nur an denselben Ursprung geschickt; HTTP-Weiterleitungen sind deaktiviert. Für ausdrücklich bestätigte Box-Zertifikate wird das exakte Zertifikat bei jeder TLS-Verbindung geprüft; ein anderes Zertifikat wird abgewiesen. Auf Unix erhalten lokale Sicherungsdateien Modus 0600 und neue Sicherungsstände Modus 0700; auf Windows gelten die geerbten Benutzer-ACLs. Ein lokaler Sicherungsstand ist kein Ersatz für eine getrennte Sicherung auf einem weiteren Datenträger.

## Stand der Prüfung

Zwölf automatisierte Tests sind unter Linux bestanden. Sie prüfen RPC-Fehler, Benutzerzuordnung ohne Administrator-Fallback, Admin-Sperre für Benutzer, Abbruch bei unvollständigen Verzeichnislisten sowie Prüfsummen und Überschreibschutz bei Wiederherstellung. Die Entwicklung erfolgt ohne Zugriff auf eine reale NAS. Schnittstellen und Berechtigungen werden aus dem ShareOS-Repository abgeleitet und lokal mit simulierten NAS-Antworten geprüft. Hardwaretests führt der Benutzer im Heimnetz durch; sie sind keine Voraussetzung für die weitere Entwicklung. `192.168.0.32` ist eine Suchadresse der lokal ausgeführten App.

Noch nicht enthalten: geplante Backups, inkrementelle Speicherung, automatische Aufbewahrung, grafische Dateiauswahldialoge, Windows-/macOS-Installer und Signierung. Für Linux liegt ein lokales Installationsskript bei. Pfade werden in dieser ersten Version vollständig in die Eingabefelder eingetragen.

## Verbindliche Grundlagen

- [ShareOS-Dokumentation](https://github.com/UniqueDroid/ShareOS/tree/main/docs), insbesondere [BACKEND.md](https://github.com/UniqueDroid/ShareOS/blob/main/docs/BACKEND.md) für JSON-RPC, Anmeldung und Tests ohne Hardware.
- [Aktuelle RPC-Implementierung](https://github.com/UniqueDroid/ShareOS/blob/main/src/shareos-core/usr/share/rpcd/ucode/shareos.uc) für `shareos.me`, `shareos.files` und `shareos.backup`.
- [RPC-Berechtigungen](https://github.com/UniqueDroid/ShareOS/blob/main/src/shareos-core/usr/share/rpcd/acl.d/shareos.json) und [Dateitransfer-CGI](https://github.com/UniqueDroid/ShareOS/blob/main/src/shareos-core/www/cgi-bin/files).
- [Vorhandene Weboberfläche](https://github.com/UniqueDroid/ShareOS/blob/main/src/shareos-core/www/app.js) als Referenz für tatsächliche Aufrufe und Wiederherstellungsvorschau.

Die Backend-Übersicht beschreibt noch einen früheren Funktionsumfang. Bei zusätzlichen Dateimethoden ist deshalb die aktuelle Implementierung maßgeblich. Es werden keine privaten NAS-Endpunkte zur Entwicklung benötigt oder abgefragt.

## HTTPS-Anmeldung mit dem Box-Zertifikat

ShareOS erzeugt beim ersten Start ein selbstsigniertes Zertifikat. Bei `UnknownIssuer`:

1. HTTPS-Box-Adresse eingeben, beispielsweise `https://shareos.skynet.lan`.
2. **HTTPS-Zertifikat prüfen** anklicken. Dieser Abruf sendet keine Zugangsdaten.
3. Den angezeigten SHA-256-Fingerabdruck mit dem Zertifikat der Box vergleichen, etwa über die Zertifikatsanzeige im Browser.
4. **Diesem Zertifikat für diese Sitzung vertrauen** anklicken und anschließend anmelden.

Die Bestätigung gilt nur für diese Box-Adresse einschließlich Port und das exakt angezeigte Zertifikat. Sie ersetzt für dieses Zertifikat auch die Namens- und Zeitprüfung, sodass der zusätzliche LAN-DNS-Name der Box verwendet werden kann. TLS-Handshakesignaturen bleiben geprüft. Es gibt keinen allgemeinen Schalter zum Abschalten der Zertifikatsprüfung für Anmeldungen. Nach Abmeldung, Adresswechsel oder Neustart muss das Zertifikat erneut bestätigt werden. Systemweit vertraute Zertifikate benötigen diese Bestätigung nicht.

## Darstellung und Linux-App-Icon

Die Oberfläche übernimmt die blauen Hell-/Dunkel-Paletten, Monospace-Schrift, Kopfzeile mit Navigation, Karten und abgerundeten Bedienelemente aus `shareos.css`. Oben rechts schaltet **Hell** / **Dunkel** den Modus um. Passend zum Modus wird das originale helle oder dunkle ShareOS-Logo angezeigt. Der Modus gilt für den aktuellen App-Start.

Für ein korrektes Symbol im Linux-Anwendungsmenü und unter Wayland:

```sh
git pull --ff-only
sh scripts/install-linux.sh
```

Das Skript benötigt Cargo, Python 3 und die üblichen Build-Abhängigkeiten. Es baut die Release-Version und installiert sie ohne sudo nach `~/.local/bin`; Desktop-Eintrag und Original-ShareOS-Icon liegen unter `${XDG_DATA_HOME:-~/.local/share}`. Anschließend die laufende App schließen und **ShareOS Desktop** aus dem Anwendungsmenü starten. Desktop-ID und Wayland-App-ID lauten `de.shareos.desktop`. Das quadratische Original-Icon stammt wie die Logos aus dem ShareOS-Repository. Beim bloßen `cargo run` ohne installierten Desktop-Eintrag kann Wayland weiterhin ein generisches Symbol zeigen.

Zusätzliche lokale Regressionstests verwenden einen echten TLS-Testserver mit selbstsigniertem Zertifikat: HTTPS-Suche, gesperrte Anmeldung ohne Vertrauen, erfolgreiche Anmeldung mit bestätigtem Zertifikat, Ablehnung eines abweichenden Zertifikats und Ursprungstrennung. Es erfolgt dabei kein Zugriff auf eine reale NAS.
