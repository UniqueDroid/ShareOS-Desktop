# ShareOS Desktop installieren und testen

ShareOS Desktop wird derzeit aus Rust-Quellcode gebaut. Es gibt noch keinen signierten Windows-Installer und keine vorgefertigte Linux-Version. Die App verändert keine NAS-Firmware. Installation und Updates benötigen GitHub-Zugriff; der spätere NAS-Zugriff läuft direkt im eigenen Netzwerk.

## Linux: Installation per curl

Benötigt werden `curl`, Git, Python 3, ein C-Compiler, `pkg-config` sowie die X11/Wayland/OpenGL-Laufzeitbibliotheken des Desktops. Native Dateidialoge benötigen `xdg-desktop-portal` und das passende Desktop-Backend. Für SMB benötigt KDE beispielsweise `kio-extras`, GNOME und ähnliche Desktops GVfs-SMB.

Debian/Ubuntu, grundlegende Build-Abhängigkeiten:

```sh
sudo apt update
sudo apt install curl git python3 build-essential pkg-config libx11-dev libxi-dev libxkbcommon-dev libgl1-mesa-dev
```

Auf anderen Distributionen die entsprechenden Pakete installieren. Anschließend als normaler Benutzer:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/UniqueDroid/ShareOS-Desktop/main/scripts/install.sh | sh
```

Alternativ erst herunterladen und anschließend ausführen:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/UniqueDroid/ShareOS-Desktop/main/scripts/install.sh -o /tmp/shareos-desktop-install.sh
sh /tmp/shareos-desktop-install.sh
```

Der Installer lädt den aktuellen Stand von `main` in ein temporäres Verzeichnis. Falls Rust/Cargo fehlen, installiert er die offizielle Rust-Werkzeugkette mit rustup im Benutzerkonto. Danach baut er die Release-Version und installiert die App, das Logo und den Menüeintrag ohne sudo:

- Programm: `~/.local/bin/shareos-desktop`
- Menüeintrag: `${XDG_DATA_HOME:-~/.local/share}/applications/de.shareos.desktop.desktop`
- Icon: `${XDG_DATA_HOME:-~/.local/share}/icons/hicolor/512x512/apps/de.shareos.desktop.png`
- Build-Cache für spätere Updates: `${XDG_CACHE_HOME:-~/.cache}/shareos-desktop/target`

Der erste Build kann einige Minuten dauern. Ein bestehendes Projektverzeichnis wird nicht verändert. Danach **ShareOS Desktop** im Anwendungsmenü starten. Alternativ `~/.local/bin/shareos-desktop` aufrufen. Die App vor einem Update schließen; zum Aktualisieren denselben curl-Befehl erneut ausführen.

Wer das Repository bereits lokal hat, kann weiterhin installieren mit:

```sh
git pull --ff-only
sh scripts/install-linux.sh
```

## Windows: lokal testen (Windows x64)

1. **Git für Windows** und **Rust mit rustup** installieren, beispielsweise in PowerShell:

   ```powershell
   winget install --id Git.Git --exact
   winget install --id Rustlang.Rustup --exact
   ```

2. Die **Visual Studio 2022 Build Tools** installieren. Im Visual Studio Installer die Workload **Desktopentwicklung mit C++** einschließlich MSVC und Windows SDK auswählen. Diese Werkzeuge sind zusätzlich zu Rust notwendig; WSL allein erzeugt keine native Windows-App.
3. PowerShell nach der Installation neu öffnen und prüfen:

   ```powershell
   git --version
   cargo --version
   rustc --version
   ```

4. Projekt laden und starten:

   ```powershell
   git clone https://github.com/UniqueDroid/ShareOS-Desktop.git
   cd ShareOS-Desktop
   cargo run --locked
   ```

Bei `link.exe not found` die C++-Workload samt Windows SDK nachinstallieren und die **Developer PowerShell for VS 2022** verwenden. Bei fehlendem `cargo` PowerShell neu starten; notfalls `& "$env:USERPROFILE\.cargo\bin\cargo.exe" run --locked` aufrufen.

Für eine ausführbare Release-Datei:

```powershell
cargo test --locked
cargo build --release --locked
.\target\release\shareos-desktop.exe
```

Die native `.exe` entsteht unter `target\release`. Die Anleitung beschreibt den Windows-Test; die Entwicklung und die bisherige Build-Prüfung erfolgten unter Linux. Ein nativer Windows-Build wurde hier noch nicht ausgeführt.

### Sinnvolle Tests an der eigenen Box

- Box automatisch finden, mit einem normalen Benutzer anmelden und dessen Dateien/Freigaben prüfen.
- Zertifikat-Fingerabdruck vor dem erstmaligen Bestätigen mit der Box vergleichen.
- Eine kleine Testdatei hochladen und wieder herunterladen.
- Unter **Dateien** eine Datei aus dem Explorer in das App-Fenster ziehen. Sie muss unter **Eigene Dateien / File Drop** auftauchen. Erneutes Ablegen derselben Datei erzeugt beispielsweise `Name (2).txt`.
- Netzlaufwerk mit einem freien Buchstaben verbinden. Windows verwendet seine eigene SMB-Anmeldung; Zugangsdaten gegebenenfalls zuerst in der Windows-Anmeldeinformationsverwaltung hinterlegen.
- Als Administrator das eingebettete Terminal und den Export der Box-Einstellungen prüfen.

Updates im vorhandenen Checkout:

```powershell
git pull --ff-only
cargo run --locked
```

## File Drop

File Drop ist im Bereich **Dateien** nach der Anmeldung verfügbar. Einzelne lokale Dateien oder mehrere ausgewählte Dateien aus Explorer, Dolphin oder einem anderen Dateimanager in das App-Fenster ziehen. Ziel ist immer der eigene Home-Ordner auf der NAS, unabhängig vom aktuell angezeigten Ordner. Falls nötig wird dort `File Drop` angelegt.

Vorhandene Dateien bleiben erhalten; Namenskonflikte werden mit Zusätzen wie ` (2)` aufgelöst. Der NAS-Upload verwendet weiterhin `overwrite:false`. Pro Datei gemeldete Fehler werden im Ergebnis angezeigt. Ohne beschreibbaren eigenen NAS-Ordner gibt es keine Ausweichablage in einer Freigabe. Ein bestehender Eintrag namens `File Drop`, der keine reguläre Directory ist, wird nicht ersetzt. Ordner und Browser-Links werden nicht hochgeladen. Während eine andere Aufgabe läuft, deren Abschluss abwarten und danach erneut ablegen.

Unter Linux mit Wayland verwendet die App für Datei-Drops automatisch XWayland, sofern `DISPLAY` vorhanden ist. Auf reinen Wayland-Systemen ohne XWayland das File-Drop-Feld anklicken und Dateien auswählen.
