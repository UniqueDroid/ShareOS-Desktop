#!/usr/bin/env python3
"""Package a native CI build; no credentials, release publishing or installer execution."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def mac_app(root, binary, version):
    app = root / "ShareOS Desktop.app"
    contents = app / "Contents"
    executable = contents / "MacOS/shareos-desktop"
    executable.parent.mkdir(parents=True)
    resources = contents / "Resources"
    resources.mkdir()
    shutil.copy2(binary, executable)
    executable.chmod(0o755)
    iconset = root / "ShareOS.iconset"
    iconset.mkdir()
    for size in (16, 32, 128, 256, 512):
        for scale, suffix in ((1, ""), (2, "@2x")):
            subprocess.run(["sips", "-z", str(size * scale), str(size * scale),
                            str(ROOT / "assets/icon-512.png"), "--out",
                            str(iconset / f"icon_{size}x{size}{suffix}.png")],
                           check=True, capture_output=True)
    subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o",
                    str(resources / "shareos.icns")], check=True)
    shutil.rmtree(iconset)
    numeric_version = version.split("-", 1)[0].split("+", 1)[0]
    with (contents / "Info.plist").open("wb") as file:
        plistlib.dump({
            "CFBundleIdentifier": "de.shareos.desktop",
            "CFBundleName": "ShareOS Desktop",
            "CFBundleDisplayName": "ShareOS Desktop",
            "CFBundleExecutable": "shareos-desktop",
            "CFBundlePackageType": "APPL",
            "CFBundleVersion": numeric_version,
            "CFBundleShortVersionString": numeric_version,
            "CFBundleIconFile": "shareos.icns",
            "LSMinimumSystemVersion": "11.0",
            "NSHighResolutionCapable": True,
            "NSLocalNetworkUsageDescription": "ShareOS Desktop connects to your NAS on the local network.",
        }, file)
    # Local ad-hoc signature; no Apple certificate or notarization is claimed.
    subprocess.run(["codesign", "--force", "--deep", "--sign", "-", str(app)], check=True)
    return app


def package(platform, target, binary, commit, output, rust_version):
    binary, output = Path(binary), Path(output)
    if not binary.is_file():
        raise FileNotFoundError(binary)
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    output.mkdir(parents=True, exist_ok=True)
    name = f"shareos-desktop-{version}-{target}-{commit[:8]}"
    extension = ".tar.gz" if platform == "linux" else ".zip"
    archive = output / (name + extension)
    with tempfile.TemporaryDirectory(prefix="shareos-package-") as temporary:
        root = Path(temporary) / "ShareOS-Desktop"
        root.mkdir()
        if platform == "macos":
            mac_app(root, binary, version)
            instructions = "Copy ShareOS Desktop.app to Applications and launch it.\n"
        else:
            destination = root / ("shareos-desktop.exe" if platform == "windows" else "shareos-desktop")
            shutil.copy2(binary, destination)
            destination.chmod(0o755)
            instructions = "Launch shareos-desktop.exe.\n" if platform == "windows" else "In a terminal: chmod +x shareos-desktop && ./shareos-desktop\n"
        shutil.copy2(ROOT / "LICENSE", root / "LICENSE")
        shutil.copy2(ROOT / "assets/icon-512.png", root / "shareos-icon.png")
        metadata = {"version": version, "commit": commit, "target": target,
                    "platform": platform, "rust": rust_version}
        (root / "BUILD-INFO.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
        (root / "START-HERE.txt").write_text(
            "ShareOS Desktop – preview build\n\n" + instructions +
            "Rust/Cargo is not required to run this application.\n"
            "Linux requires the usual X11/Wayland/OpenGL runtime libraries.\n"
            "Windows builds are unsigned. macOS bundles are ad-hoc signed and are not notarized.\n"
            "Build validation does not replace testing with your NAS.\n\n"
            "Project: https://github.com/UniqueDroid/ShareOS-Desktop\n", encoding="utf-8")
        if platform == "linux":
            with tarfile.open(archive, "w:gz") as file:
                def permissions(member):
                    # Explicit archive mode also works when tests run on Windows,
                    # where chmod cannot represent Unix executable permission bits.
                    if member.name == "ShareOS-Desktop/shareos-desktop":
                        member.mode = 0o755
                    return member
                file.add(root, arcname=root.name, filter=permissions)
        else:
            with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as file:
                for path in sorted(root.rglob("*")):
                    if path.is_file():
                        file.write(path, path.relative_to(root.parent))
    with archive.open("rb") as file:
        digest = hashlib.file_digest(file, "sha256").hexdigest()
    checksum = output / (archive.name + ".sha256")
    checksum.write_text(f"{digest}  {archive.name}\n", encoding="utf-8")
    return archive, checksum


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--platform", required=True, choices=("linux", "windows", "macos"))
    parser.add_argument("--target", required=True)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    rust = subprocess.check_output(["rustc", "--version"], text=True).strip()
    archive, checksum = package(args.platform, args.target, args.binary, args.commit, args.output, rust)
    print(archive)
    print(checksum)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as file:
            file.write(f"### ShareOS Desktop: {args.target}\n\n"
                       f"- Version: `{tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']}`\n"
                       f"- Commit: `{args.commit}`\n- Package: `{archive.name}`\n"
                       f"- Checksum: `{checksum.name}`\n")


if __name__ == "__main__":
    main()
