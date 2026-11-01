"""Installer smoke tests: no downloads, builds or writes outside temporary directories."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class InstallerTests(unittest.TestCase):
    @unittest.skipUnless(sys.platform.startswith("linux"), "Linux installer")
    def test_bootstrap_from_outside_checkout_and_reinstall(self):
        with tempfile.TemporaryDirectory(prefix="shareos-bootstrap-test-") as name:
            stage = Path(name)
            tools = stage / "tools"
            tools.mkdir()
            home = stage / "home with spaces"
            home.mkdir()
            source = stage / "fixture"
            for item in ("scripts/install-linux.sh", "assets/icon-512.png",
                         "packaging/linux/de.shareos.desktop.desktop"):
                target = source / item
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(ROOT / item, target)
            cargo = tools / "cargo"
            cargo.write_text(f"#!{sys.executable}\n" + '''import json, os, pathlib, sys
root = pathlib.Path(os.environ["CARGO_TARGET_DIR"])
if sys.argv[1] == "build":
    binary = root / "release/shareos-desktop"
    binary.parent.mkdir(parents=True, exist_ok=True)
    binary.write_text("#!/bin/sh\\nexit 0\\n")
elif sys.argv[1] == "metadata":
    print(json.dumps({"target_directory": str(root)}))
else:
    raise SystemExit("Unexpected cargo invocation")
''')
            cargo.chmod(0o755)
            git = tools / "git"
            git.write_text(f"#!{sys.executable}\n" + '''import os, shutil, sys
assert sys.argv[1:4] == ["clone", "--depth", "1"]
assert sys.argv[4] == "https://github.com/UniqueDroid/ShareOS-Desktop.git"
shutil.copytree(os.environ["SHAREOS_TEST_SOURCE"], sys.argv[5])
''')
            git.chmod(0o755)
            env = dict(os.environ, HOME=str(home), XDG_DATA_HOME=str(home / "data"),
                       XDG_CACHE_HOME=str(home / "cache"), SHAREOS_TEST_SOURCE=str(source),
                       PATH=str(tools) + os.pathsep + os.environ["PATH"])
            for _ in range(2):
                subprocess.run(["sh", str(ROOT / "scripts/install.sh")], cwd=stage,
                               env=env, check=True, capture_output=True, text=True)
            binary = home / ".local/bin/shareos-desktop"
            self.assertTrue(binary.is_file())
            desktop = home / "data/applications/de.shareos.desktop.desktop"
            self.assertIn(f'Exec="{binary}"', desktop.read_text())
            self.assertEqual((home / "data/icons/hicolor/512x512/apps/de.shareos.desktop.png").read_bytes(),
                             (ROOT / "assets/icon-512.png").read_bytes())
            self.assertTrue((home / "cache/shareos-desktop/target/release/shareos-desktop").is_file())
            if shutil.which("desktop-file-validate"):
                subprocess.run(["desktop-file-validate", str(desktop)], check=True)


if __name__ == "__main__":
    unittest.main()
