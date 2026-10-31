# ShareOS Desktop

Früher Rust-Prototyp für Windows, Linux und macOS mit egui/eframe. Linux-Build und 28 automatisierte Tests sind geprüft; die grafische Oberfläche und die plattformspezifischen Integrationen sind noch nicht durchgängig getestet. Eigenständiges Projekt im Repository [UniqueDroid/ShareOS-Desktop](https://github.com/UniqueDroid/ShareOS-Desktop) neben dem Firmware-Repository. Die Firmware bleibt unverändert. Die beiden Original-Logos stammen aus `ShareOS/src/shareos-core/www/` (ShareOS, GPLv2).

## Starten und prüfen

Rust stable mit Cargo installieren, dann im Projektverzeichnis:

```sh
cargo run --locked
cargo test --locked
cargo build --release --locked
```

Die Binärdatei liegt unter `target/release/shareos-desktop` (Windows: `.exe`). Auf jedem Zielsystem nativ bauen. Windows benötigt die MSVC-Buildtools, macOS die Xcode Command Line Tools, Linux einen C-Linker und X11/Wayland/OpenGL-Laufzeitbibliotheken. Linux-SMB benötigt einen Dateimanager mit SMB-Unterstützung, beispielsweise Dolphin mit `kio-extras` oder einen GVfs-basierten Dateimanager mit GVfs-SMB. Windows- und macOS-Builds wurden in dieser Linux-Umgebung noch nicht geprüft; für Windows und macOS gibt es noch keine Installer oder Signierung.

## Die fünf Bereiche

- **Box finden:** Ein Klick auf „Boxen suchen“ erkennt die lokalen IPv4-Adressen und Netzmasken und durchsucht deren Netze automatisch. Zusätzlich werden `shareos.local`, `192.168.0.32` und die eingegebene Box-Adresse geprüft. Die manuelle Subnetzvorgabe bleibt unter „Erweiterte Suche“ verfügbar. Manuelle Box-Adresse mit HTTP oder HTTPS und optional Port möglich. Die Suche identifiziert die bestehende ShareOS-Webanwendung; sie ist keine mDNS-Dienstsuche. HTTPS verwendet standardmäßig den Zertifikatsspeicher des Betriebssystems. Die Suche prüft HTTP und HTTPS, auch bei selbstsignierten Box-Zertifikaten; dabei werden keine Anmeldedaten gesendet. Zusätzlich wird die eingegebene Box-Adresse geprüft. Ein Suchtreffer bedeutet noch kein Zertifikatsvertrauen. Bei HTTP werden auch Anmeldedaten unverschlüsselt übertragen.
- **Dateien:** `session.login`, anschließend verpflichtend `shareos.me.bind`. Ohne erfolgreiche Benutzerzuordnung kein Zugriff und keine angenommene Administratorrolle. Ordner und Leserechte kommen aus `shareos.files.roots`; Listen über `shareos.files.list`. Downloads und Uploads verwenden kurzlebige Einmaltickets von `shareos.files.ticket` und `/cgi-bin/files`. Übertragungen laufen im Hintergrund. Bestehende lokale oder entfernte Dateien werden nicht absichtlich überschrieben. Nach der Anmeldung werden die Dateien automatisch geladen; auch nach Uploads und neuen Ordnern wird die Ansicht aktualisiert. Sitzung nach einer Stunde gegebenenfalls erneut anmelden.
- **Netzlaufwerke:** Windows `net.exe use S: \\box\freigabe /persistent:no`, macOS `open smb://…`, Linux `xdg-open smb://benutzer@box/freigabe` (ersatzweise `gio open`). Die Betriebssystem-Anmeldung ist unabhängig von der Desktop-Websitzung. Unter Windows vorher passende Zugangsdaten in der Anmeldeinformationsverwaltung speichern. Eigene SMB-Ordner und berechtigte Freigaben werden aus `shareos.me.bind` beziehungsweise `shareos.me.get` übernommen und mit Leserechten angezeigt. Manuelle Eingabe bleibt möglich. SMB-Verbindung und Berechtigungen müssen auf dem jeweiligen Zielsystem getestet werden.
- **Dateibackup:** Ausgewählte eigene Ordner und freigegebene Ordner rekursiv lokal sichern, einschließlich versteckter Dateien. Die NAS prüft bei jedem Aufruf die Benutzerrechte. Nur vollständige Durchläufe werden als `snapshot-…` sichtbar; Fehler verwerfen den unvollständigen Sicherungsstand. Bei abgeschnittenen NAS-Verzeichnislisten (mehr als 2000 Einträge) wird abgebrochen. Links und Spezialdateien werden ausgelassen und gezählt. Dateien werden nacheinander gelesen; Änderungen während des Backups sind möglich, es ist kein atomarer NAS-Snapshot. Unterverzeichnisse werden über ihre Dateien gesichert; leere Ordner, POSIX-Rechte und ACLs werden noch nicht wiederhergestellt. Sicherungsdaten bleiben unverschlüsselt auf dem lokalen Datenträger; verschlüsselten Datenträger verwenden, falls erforderlich.
- **Wiederherstellung im Dateibackup:** Sicherungsstand öffnen, lokalen Zielordner angeben und einzelne Datei wiederherstellen. SHA-256 und Dateigröße werden geprüft; bestehende Dateien werden abgewiesen. Dateien landen mit ihrem ursprünglichen Namen im gewählten Zielordner. Für gleichnamige Dateien aus verschiedenen NAS-Ordnern unterschiedliche Zielordner verwenden. Rückübertragung zur Box erfolgt anschließend über Dateien mit aktuellen Schreibrechten.
- **Box-Einstellungen:** Nur bei bestätigter Administratorrolle. `shareos.backup.export` liefert die Sicherungsdatei, optional durch die NAS mit dem angegebenen Kennwort verschlüsselt. Ein nativer Speicherdialog schlägt den Dateinamen der Box vor; ein leerer Dateipfad ist dadurch kein Fehler mehr. Das Kennwort muss leer sein oder mindestens sechs Zeichen enthalten. Bereits vorhandene Dateien werden nicht ersetzt. Wiederherstellen verwendet zuerst `shareos.backup.restore` mit `apply:false`; erst die explizite Bestätigung spielt genau den geprüften Inhalt mit `apply:true` zurück. Danach Box manuell neu starten und erneut anmelden. Kein automatischer Neustart und kein Firmware-Update.

Für Dateibackups mit dem normalen Benutzer anmelden: Die aktuelle Firmware liefert Administratoren unter `shareos.files.roots` ganze Laufwerke statt nur persönlicher Ordner und Freigaben. Deshalb ist bei Administratorkonten zunächst nichts ausgewählt und die Oberfläche weist auf den größeren Umfang hin.

Anmeldepasswörter und Sitzungstokens der Desktop-App werden nicht auf Platte gespeichert. Optional werden nur die Box-Adresse und das ausdrücklich bestätigte öffentliche Zertifikat gespeichert. Einmaltickets werden nur an denselben Ursprung geschickt; HTTP-Weiterleitungen sind deaktiviert. Für ausdrücklich bestätigte Box-Zertifikate wird das exakte Zertifikat bei jeder TLS-Verbindung geprüft; ein anderes Zertifikat wird abgewiesen. Auf Unix erhalten lokale Sicherungsdateien Modus 0600 und neue Sicherungsstände Modus 0700; auf Windows gelten die geerbten Benutzer-ACLs. Ein lokaler Sicherungsstand ist kein Ersatz für eine getrennte Sicherung auf einem weiteren Datenträger.

## Stand der Prüfung

28 automatisierte Tests sind unter Linux bestanden. Sie prüfen RPC-Fehler, Benutzerzuordnung ohne Administrator-Fallback, Admin-Sperre für Benutzer, Abbruch bei unvollständigen Verzeichnislisten sowie Prüfsummen und Überschreibschutz bei Wiederherstellung. Die Entwicklung erfolgt ohne Zugriff auf eine reale NAS. Schnittstellen und Berechtigungen werden aus dem ShareOS-Repository abgeleitet und lokal mit simulierten NAS-Antworten geprüft. Hardwaretests führt der Benutzer im Heimnetz durch; sie sind keine Voraussetzung für die weitere Entwicklung. `192.168.0.32` ist eine Suchadresse der lokal ausgeführten App.

Noch nicht enthalten: geplante Backups, inkrementelle Speicherung, automatische Aufbewahrung, Windows-/macOS-Installer und Signierung. Für Linux liegt ein lokales Installationsskript bei. Upload und Download verwenden native Dateidialoge. Die Box-Einstellungen verwenden ebenfalls Dateidialoge. Der Dateibackup-Bereich verwendet weiterhin Pfadeingabefelder. Unter Linux benötigen die Dialoge einen laufenden `xdg-desktop-portal`-Dienst mit passendem Desktop-Backend (beispielsweise KDE oder GTK).

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

Die Bestätigung gilt nur für diese Box-Adresse einschließlich Port und das exakt angezeigte Zertifikat. Sie ersetzt für dieses Zertifikat auch die Namens- und Zeitprüfung, sodass der zusätzliche LAN-DNS-Name der Box verwendet werden kann. TLS-Handshakesignaturen bleiben geprüft. Es gibt keinen allgemeinen Schalter zum Abschalten der Zertifikatsprüfung für Anmeldungen. Ohne gespeichertes Zertifikatsvertrauen muss nach Abmeldung, Adresswechsel oder Neustart erneut bestätigt werden. Mit ausdrücklich gespeichertem Vertrauen wird das genaue Zertifikat beim nächsten App-Start wieder geladen. Systemweit vertraute Zertifikate benötigen diese Bestätigung nicht.

## Darstellung und Linux-App-Icon

Die Oberfläche übernimmt die blauen Hell-/Dunkel-Paletten, Monospace-Schrift, Kopfzeile mit Navigation, Karten und abgerundeten Bedienelemente aus `shareos.css`. Oben rechts schaltet **Hell** / **Dunkel** den Modus um. Passend zum Modus wird das originale helle oder dunkle ShareOS-Logo angezeigt. Der Modus gilt für den aktuellen App-Start.

Für ein korrektes Symbol im Linux-Anwendungsmenü und unter Wayland:

```sh
git pull --ff-only
sh scripts/install-linux.sh
```

Das Skript benötigt Cargo, Python 3 und die üblichen Build-Abhängigkeiten. Es baut die Release-Version und installiert sie ohne sudo nach `~/.local/bin`; Desktop-Eintrag und Original-ShareOS-Icon liegen unter `${XDG_DATA_HOME:-~/.local/share}`. Anschließend die laufende App schließen und **ShareOS Desktop** aus dem Anwendungsmenü starten. Desktop-ID und Wayland-App-ID lauten `de.shareos.desktop`. Das quadratische Original-Icon stammt wie die Logos aus dem ShareOS-Repository. Beim bloßen `cargo run` ohne installierten Desktop-Eintrag kann Wayland weiterhin ein generisches Symbol zeigen.

Zusätzliche lokale Regressionstests verwenden einen echten TLS-Testserver mit selbstsigniertem Zertifikat: HTTPS-Suche, gesperrte Anmeldung ohne Vertrauen, erfolgreiche Anmeldung mit bestätigtem Zertifikat, Ablehnung eines abweichenden Zertifikats und Ursprungstrennung. Es erfolgt dabei kein Zugriff auf eine reale NAS.

## Dateibrowser

- Seitenleiste mit eigenen Dateien und freigegebenen Ordnern, einschließlich Nur-Lesen-Kennzeichnung.
- Klickbare Pfadnavigation, eine Ebene nach oben und Aktualisieren mit F5.
- Suche im aktuellen Ordner, versteckte Dateien ein-/ausblenden, Sortierung nach Name, Größe oder Änderung. Ordner stehen immer oben.
- Dateiliste mit gezeichneten Ordner-/Dateisymbolen, Dateityp, KiB/MiB/GiB-Größen, relativen Änderungshinweisen und Eintragszahl.
- **Hochladen …** öffnet die native Dateiauswahl, **Speichern …** den Speicherdialog. Bestehende Dateien werden weiterhin abgewiesen, auch wenn der Speicherdialog ein Überschreiben anbietet.
- Uploads und Downloads zeigen die verarbeiteten Bytes; die Erfolgsmeldung erscheint erst nach der NAS-Antwort beziehungsweise dem vollständigen lokalen Schreiben. Beim Upload bedeutet 100 % zunächst, dass der HTTP-Client alle Bytes gelesen hat; die Serverbestätigung folgt anschließend.
- **Neuer Ordner** ist bei Schreibrechten verfügbar. Nach Upload und Ordneranlage aktualisiert sich die Liste automatisch.

## Admin-Terminal

Nur bei bestätigter Administratorrolle erscheint **Terminal** als zusätzlicher Bereich. **Terminal verbinden** startet die vorhandene Methode `shareos.system.terminal`, holt den ttyd-Authentifizierungstoken und verbindet sich mit dessen WebSocket-Protokoll `tty`. Das Terminal wird direkt im nativen App-Fenster dargestellt; es öffnet sich kein Browser. Normale Benutzer können die Terminal-Methode nicht über die App starten; die NAS-ACLs bleiben ebenfalls wirksam.

Die VT100-Ansicht unterstützt Farben, Cursorbewegungen, Größenänderungen, Unicode, Pfeiltasten, Tab, Enter, Strg+C und Einfügen (bei Bedarf mit dem Bracketed-Paste-Protokoll). Zum Tippen ins Terminal klicken. Scrollback ist auf 2000 Zeilen begrenzt, der sichtbare Bildschirm lässt sich kopieren. Einfügen verwendet die Betriebssystem-Tastenkombination, unter Linux üblicherweise Strg+Umschalt+V. Grafische Terminalprotokolle wie Sixel und Terminal-Maussteuerung sind nicht implementiert.

**Terminal beenden**, Abmelden und Schließen der App schließen die WebSocket-Sitzung. Ein Wechsel zu einem anderen App-Bereich lässt sie weiterlaufen. Eine neue Terminal-Sitzung ersetzt laut ShareOS-Implementierung eine vorhandene Sitzung. Bei HTTPS gelten der System-Zertifikatsspeicher beziehungsweise das bereits bestätigte Box-Zertifikat auch für die Terminal-Verbindung zum gleichen Host am von der Box gelieferten Port. Der Sitzungspfad wird weder gespeichert noch absichtlich ausgegeben.

## Box-Adresse und Zertifikat merken

Unter **Box finden → Verbindung merken** können die Box-Adresse und optional das bestätigte Zertifikat gespeichert werden. Nach Auswahl auf **Verbindungseinstellungen speichern** klicken. **Gespeicherte Verbindung vergessen** entfernt die gespeicherten Werte. Ein anderes Zertifikat wird auch nach dem Speichern abgewiesen und muss separat geprüft werden.

Die Datei `shareos-desktop/connection.json` liegt unter Linux in `${XDG_CONFIG_HOME:-~/.config}`, unter macOS in `~/Library/Application Support` und unter Windows in `%APPDATA%`. Sie enthält keine Anmeldedaten. Auf Unix werden neue Konfigurationsdateien mit 0600 und der App-Konfigurationsordner mit 0700 angelegt.

## Korrekturen anhand der Linux-Screenshots

Die Ordnerauswahl ist ausdrücklich vertikal angeordnet, sodass sich eigene Dateien und Laufwerke nicht mehr überlagern. Benutzername, Rolle und Box-Adresse stehen beim Logo im Kopfbereich; Abmelden steht rechts. Eingabefelder haben gleichmäßige Höhe und Abstände. Bei bestehender Anmeldung werden die überflüssigen Passwortfelder ausgeblendet.

Zusätzliche Tests prüfen Speichern der Einstellungen, Benutzerfreigaben, gespeichertes Zertifikatsvertrauen, vertikale Navigation und das ttyd-Protokoll mit einem lokalen WebSocket-Testserver einschließlich Eingaben, Ausgabe und Sitzungsende. Die reale NAS wird aus der Entwicklungsumgebung nicht angesprochen.

## Automatische lokale Suche

Ohne manuelle Subnetzvorgabe werden die IPv4-Adressen und Netzmasken der lokalen Netzwerkschnittstellen ausgelesen. Die Schnittstelle der Standardroute wird zuerst berücksichtigt; weitere LAN-Schnittstellen bleiben etwa bei aktivem VPN enthalten. Loopback, Link-Local sowie erkennbare Docker-/Veth-/Bridge-Schnittstellen werden ausgelassen. Die Ermittlung der Standardroute verwendet eine UDP-Routenauswahl ohne Paketversand.

Bis /22 wird das erkannte Netz durchsucht. Größere Netze werden auf den /24-Adressbereich der jeweiligen eigenen IPv4-Adresse begrenzt, um keine sehr großen Netze vollständig zu scannen. Jeder Suchkandidat wird über HTTP und HTTPS geprüft. Die bekannte Box-Adresse und `shareos.local` bleiben auch bei fehlender IPv4-Erkennung als Suchziele enthalten. Die Suche wird ausschließlich durch den Benutzer in der lokal gestarteten App ausgelöst; Tests prüfen die Adressberechnung ohne Zugriff auf eine NAS.

## Linux-Dateidialoge und Navigation

Die synchronen XDG-Portal-Dateidialoge verwenden das `async-std`/`async-io`-Backend von rfd/ashpd/zbus. Dadurch benötigen die Hintergrundthreads der App keine eigene Tokio-Laufzeit. Ein isolierter Regressionstest ruft Dateiöffnung und Speicherdialog ohne Tokio, ohne erreichbaren D-Bus und ohne grafischen Fallback auf; fehlende Dienste führen dabei zu einem regulären Abbruch statt zum Reactor-Panic.

Vor der Anmeldung ist ausschließlich **Box finden** sichtbar. Nach erfolgreicher Anmeldung werden die weiteren Bereiche eingeblendet. Das Terminal bleibt zusätzlich auf Administratoren beschränkt. Beim Abmelden wird die aktive Ansicht auf Box finden zurückgesetzt.
