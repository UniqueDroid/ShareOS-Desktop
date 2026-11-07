# Changelog

Changes per release (newest first).

## 0.2.2

- **Switch from Alt-F:** *Box einrichten* refused every Alt-F box with "Alt-F ist noch nicht eingerichtet (kein Passwort gesetzt)". The check looked for `passwd_again` anywhere on the login page, but Alt-F's login script names it on every page; now only the form field of the first-time password counts. Tests use Alt-F's real login page.

## 0.2.1

- **Settings backups with ShareOS 0.2.0-beta.20 and older:** *Jetzt sichern* wrote no file – the app always sent the new certificate/CA options, which older firmware refuses (`RPC verweigert (Code 2)`). They are now sent only when ticked (only offered by beta.21 and newer).
- Messages after *Jetzt sichern* (saved or error) stay visible; the backup list reloaded right after and replaced them.

## 0.2.0

- **Box setup wizard** (tab *Box einrichten*), like the vendor's setup tool: new administrator password (required while the factory password `shareos` is set), box name, time zone, update channel, optional Alt-F import; existing data partitions are taken over unchanged, new disks are formatted (single or mirror) only after typing `LÖSCHEN`; a first regular user with a share; then the login.
- **Switch from Alt-F to ShareOS:** discovery recognises Alt-F boxes. The app logs in to Alt-F, downloads the newest ShareOS release for the model from GitHub (stable or beta), verifies SHA-256 and SHA-1, lets Alt-F check the image and flashes only after a confirmation page. It waits for ShareOS and continues with the setup wizard. Data disks, U-Boot and the D-Link rescue system are not touched.
- **Settings backups overview** (*Box-Einstellungen*): backups in a folder on this computer (default `~/ShareOS-Sicherungen`) with date, box, version and encryption – newest 3, *Alle anzeigen* for the rest. *Jetzt sichern*, restore with a preview. With a password the HTTPS certificate and the own CA can be included.
- **Recovery mode** (ShareOS 0.2.0-beta.21 or newer): after a restore the box takes over what it needs to run and restarts; logging in as `root` opens the selection of what else comes back – users and groups, shares, services, apps, certificate, own CA.
- **Certificates:** saved trust survives a logout, so another user can log in after `root` (before: `UnknownIssuer`). A login that fails on an unknown certificate shows its fingerprint with *Vertrauen und auf diesem Rechner merken* / *Nur für diese Sitzung vertrauen* and then logs in. The setup wizard asks the same way over HTTPS.
- **Connecting:** the first tab is now *Verbinden* with the login first; the network search folds away when an address is saved; Enter logs in; the box session is kept alive while logged in. Regular users only see their own tabs.
- Long texts no longer run out of the window, and the theme button stays visible in smaller windows.

## 0.1.0

First public preview: discovery, login with certificate checks, file browser, File Drop, SMB shares, local file backups, settings backup and restore, administrator terminal, light and dark themes.
