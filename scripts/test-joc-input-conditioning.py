#!/usr/bin/env python3
"""Run the shared E-AC-3 library tests without the unrelated bridge workspace.

The upstream workspace's bridge member requires an external Omniphony checkout.
Copy only this library's exact source and manifest into a temporary standalone
workspace; no source edits, fixture downloads, or production configuration changes.
Extra arguments are passed to cargo test (e.g. --offline or --target ...).
"""
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[1]
source = root / "harletty-bridge/eac3"
with tempfile.TemporaryDirectory(prefix="sda-joc-tests-") as temporary:
    dest = Path(temporary)
    shutil.copytree(source / "src", dest / "src")
    shutil.copytree(source / "tests", dest / "tests")
    shutil.copyfile(source / "README.md", dest / "README.md")
    shutil.copyfile(source / "JOC_INPUT_CONDITIONING_LICENSE", dest / "JOC_INPUT_CONDITIONING_LICENSE")
    manifest = (source / "Cargo.toml").read_text(encoding="utf-8")
    (dest / "Cargo.toml").write_text(manifest + "\n[workspace]\n", encoding="utf-8")
    subprocess.run(["cargo", "test", "--manifest-path", str(dest / "Cargo.toml"),
                    "--lib", *sys.argv[1:]], check=True)
