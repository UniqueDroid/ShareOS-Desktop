# ShareOS Desktop

An early Rust desktop application for Windows, Linux, and macOS, built with egui/eframe. This is a standalone project alongside [ShareOS](https://github.com/UniqueDroid/ShareOS); it does not modify the NAS firmware. Original logos and icons come from `ShareOS/src/shareos-core/www/` (GPLv2).

## Downloads

Packages for **Linux x64**, **Windows x64**, **macOS Apple Silicon**, and **macOS Intel** are available under **[Releases](https://github.com/UniqueDroid/ShareOS-Desktop/releases)** once that version's builds finish. Rust is not required to run them. See [Installation and testing](docs/INSTALL.md#prebuilt-packages).

## Quick Linux installation

Install the [build dependencies](docs/INSTALL.md#linux-installation-with-curl), then run:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/UniqueDroid/ShareOS-Desktop/main/scripts/install.sh | sh
```

The installer downloads the source, installs Rust for your user account if needed, and builds locally. Run the same command to update. See [Installation and testing](docs/INSTALL.md) for installation paths, requirements, and Windows instructions.

## Build and check

Install stable Rust with Cargo, then run in the project directory:

```sh
cargo run --locked
cargo test --locked
cargo build --release --locked
```

The executable is `target/release/shareos-desktop` (`shareos-desktop.exe` on Windows). Build natively on each target system. Windows requires the MSVC build tools, macOS requires Xcode Command Line Tools, and Linux requires a C linker and X11/Wayland/OpenGL libraries. Linux SMB access requires an SMB-capable file manager, such as Dolphin with `kio-extras` or a GVfs-based file manager with GVfs-SMB.

## Features

- **Box discovery:** Detect local IPv4 addresses and network masks and search their networks automatically. Also check `shareos.local`, `192.168.0.32`, and the entered box address. An optional manual subnet is available in advanced search. HTTP/HTTPS URLs and custom ports are supported. Discovery identifies the ShareOS web application; it is not mDNS service discovery. A discovery result does not establish certificate trust. HTTP transmits credentials without encryption.
- **Box setup (new box):** The "Box einrichten" tab sets up a new ShareOS box like the vendor's setup tool: it tries the factory password `shareos`, then asks for a new administrator password (required while the factory password is still set), the box name, time zone and update channel, and optionally imports Alt-F users and shares (`shareos.setup apply`). Existing partitions with data are taken over unchanged (`shareos.storage adopt`); formatting new disks (single or mirror) needs the word `LÖSCHEN` typed in. Finally it creates a first regular user with a share of the new drive (`shareos.accounts add`, `shareos.shares set`) and hands over to the login.
- **Switch from Alt-F to ShareOS:** Discovery also recognises Alt-F boxes (`· Alt-F`) and offers "auf ShareOS umstellen". The app logs in to the Alt-F web interface, downloads the newest ShareOS release for the selected model from GitHub (stable or beta), verifies SHA-256 and SHA-1, and lets Alt-F check the image (`firmware_proc.cgi`, which also verifies the box model). Nothing is flashed before the user confirms. Then Alt-F flashes kernel and initramfs (`FlashIt`, after a special reboot when Alt-F runs from flash), the box reboots, ShareOS installs its system (USB stick or internet) and the app continues with the box setup above. Data disks, U-Boot and the D-Link rescue system are not touched.
- **Files:** Log in with `session.login`, then require `shareos.me.bind` before accessing files. No administrator role is assumed if user binding fails. Roots and permissions come from `shareos.files.roots`; listings use `shareos.files.list`. Transfers use short-lived, single-use tickets from `shareos.files.ticket` and `/cgi-bin/files`. Transfers run in the background and do not intentionally overwrite existing files. The view refreshes after login, uploads, and folder creation. Sessions may require a new login after one hour.
- **Network drives:** Available personal SMB folders and authorized shares come from `shareos.me.bind` or `shareos.me.get`, including read permissions. Manual entry remains available. Windows uses `net.exe use`, macOS uses `open smb://…`, and Linux uses `xdg-open smb://…` with `gio open` as a fallback. SMB authentication belongs to the operating system and is separate from the desktop web session. On Windows, save suitable credentials in Credential Manager first. Test connections and permissions on each target OS.
- **File backup:** Recursively back up selected personal folders and shared folders locally, including hidden files. The NAS checks user permissions on every call. Only complete runs become visible as `snapshot-*`; failures discard incomplete runs. Truncated directory listings (more than 2,000 entries) cause the backup to stop. Links and special files are skipped and counted. Files are read sequentially, so changes during backup are possible: this is not an atomic NAS snapshot. Empty folders, POSIX permissions, and ACLs are not restored. Backup data is stored unencrypted; use an encrypted drive if required.
- **File restoration:** Select a snapshot, choose a separate local output folder, and restore individual files. SHA-256 and file size are verified; existing files are rejected. Restored files use their original names. Use separate output folders for identically named files from different NAS folders. Upload restored files through the file browser if needed, subject to current write permissions.
- **Box settings:** Available only to confirmed administrators. `shareos.backup.export` produces the settings backup, optionally encrypted by the NAS with a password. A native save dialog suggests the NAS filename. The password must be empty or at least six characters long. Existing files are not replaced. Restoration first previews `shareos.backup.restore` with `apply:false`; explicit confirmation applies exactly the checked content with `apply:true`. Restart the box manually afterward and log in again. This does not update firmware or restart the NAS automatically.

Use a regular user account for personal file backups. The current firmware exposes entire drives through `shareos.files.roots` for administrators, rather than only personal folders and shares. Administrator accounts therefore start with nothing selected and receive a scope notice.

## File browser

The sidebar lists personal files and shared folders, including read-only status. Breadcrumbs, parent-folder navigation, F5 refresh, folder search, hidden-file visibility, and sorting by name, size, or modification time are supported. Folders remain at the top. Entries display file icons, type, KiB/MiB/GiB sizes, relative modification times, and an entry count.

Uploads and downloads use native file dialogs and report processed bytes. Success is reported only after the NAS response or complete local write. Upload progress reaching 100% means the HTTP client has read all bytes; server confirmation follows. Existing files remain protected even if a save dialog offers to overwrite them.

Checkboxes select individual entries or all visible entries. The toolbar and context menu offer copy, paste, rename, and delete. New file creates an empty file; new folder creates a directory. Rename requires exactly one selected entry. Write actions are disabled for read-only roots and additionally checked by the NAS.

Copy stores the selection in an internal clipboard. Open the destination and paste. Within one root, copying uses `shareos.files.move` with `copy:true`. Across roots, the app plans the directory tree and stages each file through a protected local temporary file; local space for the largest file is required. Empty folders are preserved during copying, but POSIX metadata is not. Links and special files can only be copied within the same root. Existing destinations are rejected. A failed copy may leave created folders or copied files; these are not automatically deleted. The clipboard is bound to the box and user and cleared at logout.

Deletion requires confirmation of the selected entries. The app sends `permanent:false`; NAS configuration determines whether its recycle bin is used or files are permanently removed. Deleting a folder includes its contents.

## File Drop

After login, drag local files into the Files view or click the File Drop field to select them. The app creates **File Drop** in your personal NAS home if needed, regardless of the folder currently being viewed. Conflicts receive suffixes such as ` (2)`; existing files are not overwritten. After upload, the browser opens the destination. Folder uploads are not supported. See [File Drop](docs/INSTALL.md#file-drop).

On Linux, the current winit Wayland backend does not deliver file-drop events. The app therefore uses X11/XWayland automatically when `DISPLAY` is set. Without XWayland, click the field to choose files instead. The same click-to-select fallback is available on other platforms.

## Backup sources and snapshots

The backup destination folder also acts as the default source of saved snapshots for restoration. Selecting it or completing a backup loads complete `snapshot-*` entries, sorted by date in UTC. A single snapshot folder can also be selected directly. Incomplete `.incomplete-*` entries are ignored; invalid `snapshot-*` entries are counted and skipped.

Enable the alternative restore-source option to select another backup folder. Disabling it makes the source follow the backup destination again. Changing the source discards the previously loaded snapshot to avoid restoring from stale data.

The output folder for restored files is selected separately and is not changed when switching backup sources. Restoration continues to verify SHA-256 and reject existing local files.

## HTTPS certificates and saved connections

ShareOS generates a self-signed certificate on first startup. If login reports `UnknownIssuer`:

1. Enter the HTTPS box address, such as `https://shareos.skynet.lan`.
2. Click the certificate inspection button. This request sends no credentials.
3. Compare the displayed SHA-256 fingerprint with the box certificate, for example using your browser's certificate viewer.
4. Explicitly trust that certificate for the session, then log in.

Trust is restricted to the exact certificate and box address, including its port. Explicit pinning replaces name and validity checks for that certificate, allowing an additional LAN DNS name; TLS handshake signatures are still verified. There is no general login option to disable certificate verification. Without saved trust, confirm again after logout, address changes, or restart. Explicitly saved trust is loaded at the next app startup. System-trusted certificates require no manual confirmation.

The connection preferences can save the box address and optionally its confirmed public certificate. Select the desired options and save the connection settings. Forgetting the saved connection removes these values. A different certificate is rejected and must be inspected separately.

`shareos-desktop/connection.json` is stored under `${XDG_CONFIG_HOME:-~/.config}` on Linux, `~/Library/Application Support` on macOS, and `%APPDATA%` on Windows. It contains no passwords or session tokens. New Unix configuration files use mode 0600 and the app configuration directory uses 0700.

Login passwords and session tokens are never saved to disk. Transfer tickets are sent only to the same origin; HTTP redirects are disabled. Pinned certificates are checked on every TLS connection. Local Unix backup files use mode 0600 and new snapshots use 0700; Windows uses inherited user ACLs. A local snapshot does not replace a separate backup on another drive.

## Embedded administrator terminal

The Terminal section appears only for confirmed administrators. Connecting invokes `shareos.system.terminal`, obtains a ttyd authentication token, and connects with the `tty` WebSocket subprotocol. The terminal is embedded in the native app. Regular users cannot invoke it through the app; NAS ACLs also remain effective.

The VT100 view supports colors, cursor movement, resizing, Unicode, arrow keys, Tab, Enter, Ctrl+C, and paste, including bracketed paste where required. Click the terminal to type. Scrollback is limited to 2,000 lines, and the visible screen can be copied. Paste uses the OS shortcut, usually Ctrl+Shift+V on Linux. Sixel and terminal mouse reporting are not implemented.

Disconnecting, logging out, or closing the app closes the WebSocket session. Switching app sections leaves it running. Starting another terminal replaces an existing session according to the ShareOS implementation. HTTPS connections use system trust or the confirmed box certificate for the same host at the returned terminal port. The session path is neither stored nor intentionally logged.

## Appearance and Linux desktop integration

The app follows `shareos.css`: blue light/dark palettes, monospace text, header navigation, cards, and rounded controls. The top-right theme button switches modes and selects the matching original ShareOS logo for the current app session. Username, role, and box address appear beside the logo; logout is on the right. Login fields are hidden while signed in. Only box discovery is shown before login; the remaining sections appear afterward.

To install the Linux menu entry and icon from an existing checkout:

```sh
git pull --ff-only
sh scripts/install-linux.sh
```

The script requires Cargo, Python 3, and the build dependencies. It installs without sudo into `~/.local/bin`, with desktop files and the icon under `${XDG_DATA_HOME:-~/.local/share}`. Close the running app and launch ShareOS Desktop from the application menu. The desktop ID and Wayland app ID are `de.shareos.desktop`. Plain `cargo run` without an installed desktop entry may still display a generic icon under Wayland.

Linux native file dialogs require a running `xdg-desktop-portal` service with the appropriate desktop backend. Synchronous dialogs use rfd/ashpd/zbus with the `async-std`/`async-io` backend, so background threads do not require their own Tokio runtime.

## Validation and limitations

The 53 Rust tests cover the setup wizard (disk plan, typed confirmation, password and name checks), the Alt-F flow against a local test server (salted login, multipart upload with SHA-1, flash and reboot, wrong password, failed flash), release selection and checksum verification, RPC failures, required user binding, administrator restrictions, truncated directory listings, backup checksums, overwrite protection, saved trust, discovery address calculation, and terminal protocol behavior. Local TLS tests check self-signed certificate discovery, blocked untrusted login, confirmed trust, certificate mismatch rejection, and origin separation. A local WebSocket server checks terminal input, output, and session closure. An isolated dialog test checks cancellation without Tokio or reachable desktop services.

Development and automated tests do not access a real NAS. APIs and permissions are derived from the ShareOS repository and tested with simulated responses. Native CI builds have succeeded on Linux, Windows, and both macOS architectures; this does not establish complete GUI or NAS integration coverage on every OS. Hardware testing takes place on the user's local network. `192.168.0.32` is a discovery candidate used by the locally running app.

Not yet included: scheduled backups, incremental storage, automatic retention, Windows/macOS installers, Windows signing, or Apple developer signing and notarization. Linux has a local installation script.

Installer and packaging smoke tests, without downloads or real compilation:

```sh
python3 -m unittest discover -s tests -v
```

## API references

- [ShareOS documentation](https://github.com/UniqueDroid/ShareOS/tree/main/docs), especially [BACKEND.md](https://github.com/UniqueDroid/ShareOS/blob/main/docs/BACKEND.md) for JSON-RPC, authentication, and hardware-free testing.
- [Current RPC implementation](https://github.com/UniqueDroid/ShareOS/blob/main/src/shareos-core/usr/share/rpcd/ucode/shareos.uc) for `shareos.me`, `shareos.files`, and `shareos.backup`.
- [RPC permissions](https://github.com/UniqueDroid/ShareOS/blob/main/src/shareos-core/usr/share/rpcd/acl.d/shareos.json) and [file transfer CGI](https://github.com/UniqueDroid/ShareOS/blob/main/src/shareos-core/www/cgi-bin/files).
- [Existing web interface](https://github.com/UniqueDroid/ShareOS/blob/main/src/shareos-core/www/app.js) for actual calls and restore previews.

The backend overview describes an older feature set. The current implementation is authoritative for additional file methods. Development does not require or query private NAS endpoints.
