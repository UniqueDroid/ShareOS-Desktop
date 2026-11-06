# Install and test ShareOS Desktop

Build ShareOS Desktop from Rust source or download a prebuilt package from Releases. There is no signed Windows installer yet. The app does not modify NAS firmware. Installation and updates require access to GitHub; subsequent NAS connections take place directly on your local network.

## Prebuilt packages

Download and extract the appropriate archive from **[Releases](https://github.com/UniqueDroid/ShareOS-Desktop/releases)**. Filenames include the version, target architecture, and commit:

- Linux x64: target `x86_64-unknown-linux-gnu`, `.tar.gz` archive.
- Windows x64: target `x86_64-pc-windows-msvc`, `.zip` archive.
- macOS Apple Silicon: target `aarch64-apple-darwin`, `.zip` archive.
- macOS Intel: target `x86_64-apple-darwin`, `.zip` archive.

Each archive has a matching SHA-256 checksum file. Packages contain the program, `START-HERE.txt`, build information, license, and icon. Older packages may call the instructions file `STARTEN.txt`.

**Windows:** Launch `shareos-desktop.exe`. Rust and Visual Studio are not required for a prebuilt package. Windows binaries are unsigned.

**Linux:** Make the executable runnable and launch it:

```sh
chmod +x shareos-desktop
./shareos-desktop
```

The usual X11/Wayland/OpenGL runtime libraries are required. To install an application menu entry, you can use the source installer below instead.

**macOS:** Copy `ShareOS Desktop.app` to Applications and launch it. The app is ad-hoc signed, without an Apple developer signature or notarization, so macOS may display a security notice. The deployment target is macOS 11 or later. A successful build does not establish full GUI or NAS integration coverage on every OS version.

## Linux: installation with curl

Requirements: `curl`, Git, Python 3, a C compiler, `pkg-config`, and your desktop's X11/Wayland/OpenGL runtime libraries. Native file dialogs require `xdg-desktop-portal` and the matching desktop backend. SMB access requires, for example, `kio-extras` for KDE or GVfs-SMB for GNOME and similar desktops.

Basic build dependencies on Debian/Ubuntu:

```sh
sudo apt update
sudo apt install curl git python3 build-essential pkg-config libx11-dev libxi-dev libxkbcommon-dev libgl1-mesa-dev
```

On other distributions, install the equivalent packages. Then run as your regular user:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/UniqueDroid/ShareOS-Desktop/main/scripts/install.sh | sh
```

Alternatively, download the script before running it:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/UniqueDroid/ShareOS-Desktop/main/scripts/install.sh -o /tmp/shareos-desktop-install.sh
sh /tmp/shareos-desktop-install.sh
```

The installer downloads the current `main` source into a temporary directory. If Rust/Cargo is missing, it installs the official toolchain with rustup for your user account. It then builds the release executable and installs the app, logo, and menu entry without sudo:

- Executable: `~/.local/bin/shareos-desktop`
- Menu entry: `${XDG_DATA_HOME:-~/.local/share}/applications/de.shareos.desktop.desktop`
- Icon: `${XDG_DATA_HOME:-~/.local/share}/icons/hicolor/512x512/apps/de.shareos.desktop.png`
- Build cache for updates: `${XDG_CACHE_HOME:-~/.cache}/shareos-desktop/target`

The first build may take several minutes. Existing project directories are left untouched. Launch ShareOS Desktop from the application menu or run `~/.local/bin/shareos-desktop`. Close the app before updating; run the same curl command again to update.

From an existing repository checkout:

```sh
git pull --ff-only
sh scripts/install-linux.sh
```

## Windows: build and test locally (x64)

1. Install **Git for Windows** and **Rust with rustup**, for example in PowerShell:

   ```powershell
   winget install --id Git.Git --exact
   winget install --id Rustlang.Rustup --exact
   ```

2. Install **Visual Studio 2022 Build Tools**. In Visual Studio Installer, select **Desktop development with C++**, including MSVC and the Windows SDK. These tools are required in addition to Rust. WSL alone does not produce the native Windows application.
3. Open a new PowerShell window and check:

   ```powershell
   git --version
   cargo --version
   rustc --version
   ```

4. Clone and run the project:

   ```powershell
   git clone https://github.com/UniqueDroid/ShareOS-Desktop.git
   cd ShareOS-Desktop
   cargo run --locked
   ```

If `link.exe` is missing, install the C++ workload and Windows SDK, then use **Developer PowerShell for VS 2022**. If `cargo` is missing, restart PowerShell or run `& "$env:USERPROFILE\.cargo\bin\cargo.exe" run --locked`.

To test and build a release executable:

```powershell
cargo test --locked
cargo build --release --locked
.\target\release\shareos-desktop.exe
```

The native executable is written to `target\release`. A native Windows CI build has passed, but testing on your own box is still needed.

### Suggested checks on your box

- Discover the box automatically, log in as a regular user, and check personal files and authorized shares.
- Compare the certificate fingerprint with the box before trusting it for the first time.
- Upload and download a small test file.
- In the Files section, drag a file from Explorer into the app. It should appear in your personal NAS home under `File Drop`. Dropping it again should produce a name such as `Name (2).txt`.
- Connect a network drive using an unused drive letter. Windows uses its own SMB authentication; save credentials in Windows Credential Manager if needed.
- As an administrator, check the embedded terminal and settings backup export.
- Box setup: on a freshly installed box, open "Box einrichten", enter the address and click "Box prüfen". Set a new administrator password, take over the existing data partition and create a regular user. Then log in with that user.
- Alt-F switch (only when you really want to replace Alt-F; this flashes the box): in "Box finden", search, click "auf ShareOS umstellen" next to the Alt-F box, log in with the Alt-F password, choose the model and let the app load the firmware. Read the confirmation page (Alt-F's check result, firmware version, SHA-256) before ticking "Verstanden" and clicking "Jetzt umstellen". Do not switch the box off; the app waits up to 30 minutes until ShareOS answers and then continues with the box setup.

To update an existing checkout:

```powershell
git pull --ff-only
cargo run --locked
```

## File Drop

File Drop is available in the Files section after login. Drag one or more local files from Explorer, Dolphin, or another file manager into the app. The destination is always your personal NAS home, regardless of the currently displayed folder. A `File Drop` folder is created there if necessary. You can also click the File Drop field to select files.

Existing files are preserved. Conflicts receive suffixes such as ` (2)`, and uploads use `overwrite:false`. Per-file errors are included in the result. If your personal NAS folder is not writable, the app does not fall back to another share. An existing `File Drop` entry that is not a regular directory is not replaced. Folders and browser links are not uploaded. Wait for any running task to finish before dropping more files.

On Linux with Wayland, the app uses XWayland for file drops when `DISPLAY` is available. On Wayland-only systems without XWayland, click the File Drop field to select files instead.
