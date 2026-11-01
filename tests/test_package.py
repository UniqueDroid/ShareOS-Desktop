"""Validate package contents and checksums without a compiler or platform tools."""
import hashlib
import importlib.util
import json
from pathlib import Path
import plistlib
import tarfile
import tempfile
import unittest
from unittest import mock
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("package_build", ROOT / "scripts/package-build.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class PackageTests(unittest.TestCase):
    def test_native_archives_contain_only_expected_payload_and_matching_checksum(self):
        for platform, target in (("linux", "x86_64-unknown-linux-gnu"),
                                 ("windows", "x86_64-pc-windows-msvc")):
            with self.subTest(platform=platform), tempfile.TemporaryDirectory() as name:
                stage = Path(name)
                binary = stage / "dummy-binary"
                binary.write_bytes(b"test executable")
                archive, checksum = MODULE.package(platform, target, binary,
                                                   "0123456789abcdef", stage / "out", "rustc test")
                self.assertIn(hashlib.sha256(archive.read_bytes()).hexdigest(), checksum.read_text())
                if platform == "linux":
                    with tarfile.open(archive) as file:
                        item = file.getmember("ShareOS-Desktop/shareos-desktop")
                        self.assertTrue(item.mode & 0o111)
                        self.assertEqual(file.extractfile(item).read(), binary.read_bytes())
                        metadata = json.load(file.extractfile("ShareOS-Desktop/BUILD-INFO.json"))
                else:
                    with zipfile.ZipFile(archive) as file:
                        self.assertEqual(file.read("ShareOS-Desktop/shareos-desktop.exe"), binary.read_bytes())
                        metadata = json.loads(file.read("ShareOS-Desktop/BUILD-INFO.json"))
                self.assertEqual(metadata["target"], target)
                self.assertEqual(metadata["commit"], "0123456789abcdef")

    def test_macos_bundle_metadata_and_icon_signing_commands(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            binary = root / "dummy-binary"
            binary.write_bytes(b"Mach-O fixture")
            with mock.patch.object(MODULE.subprocess, "run") as run:
                app = MODULE.mac_app(root, binary, "0.1.0-beta.1")
            with (app / "Contents/Info.plist").open("rb") as file:
                metadata = plistlib.load(file)
            self.assertEqual(metadata["CFBundleIdentifier"], "de.shareos.desktop")
            self.assertEqual(metadata["CFBundleVersion"], "0.1.0")
            self.assertEqual((app / "Contents/MacOS/shareos-desktop").read_bytes(), binary.read_bytes())
            calls = [call.args[0] for call in run.call_args_list]
            self.assertTrue(any(call[0] == "iconutil" for call in calls))
            self.assertTrue(any(call[:5] == ["codesign", "--force", "--deep", "--sign", "-"] for call in calls))


if __name__ == "__main__":
    unittest.main()
