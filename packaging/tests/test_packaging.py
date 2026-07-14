from __future__ import annotations

import hashlib
import io
import json
from pathlib import Path
import re
import sys
import tarfile
import tempfile
import unittest
import xml.etree.ElementTree as ET
import zipfile

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.9-3.10 packaging smoke
    from pip._vendor import tomli as tomllib


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "packaging"))

from render_release import ASSETS, render  # noqa: E402
from verify_release import verify  # noqa: E402


class PackagingMetadataTests(unittest.TestCase):
    def test_versions_and_commands_match_workspace(self) -> None:
        cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
        version = cargo["workspace"]["package"]["version"]
        npm = json.loads((ROOT / "pkg/npm/package.json").read_text(encoding="utf-8"))
        pypi = tomllib.loads((ROOT / "pkg/pypi/pyproject.toml").read_text(encoding="utf-8"))
        go = (ROOT / "pkg/go/cmd/goldeneye/main.go").read_text(encoding="utf-8")

        self.assertEqual(npm["version"], version)
        self.assertEqual(pypi["project"]["version"], version)
        self.assertRegex(go, rf'defaultVersion\s*=\s*"{re.escape(version)}"')
        self.assertEqual(npm["bin"], {"goldeneye": "bin.js"})
        self.assertEqual(pypi["project"]["scripts"]["goldeneye"], "goldeneye_tool._cli:main")

    def test_package_legal_files_are_exact_copies(self) -> None:
        license_bytes = (ROOT / "LICENSE").read_bytes()
        notice_bytes = (ROOT / "NOTICE").read_bytes()
        for directory in (ROOT / "pkg/npm", ROOT / "pkg/pypi", ROOT / "pkg/chocolatey"):
            self.assertEqual((directory / "LICENSE").read_bytes(), license_bytes, directory)
            self.assertEqual((directory / "NOTICE").read_bytes(), notice_bytes, directory)

    def test_templates_and_nuspec_are_well_formed(self) -> None:
        nuspec = ROOT / "pkg/chocolatey/goldeneye-tool.nuspec.tmpl"
        ET.parse(nuspec)
        for template in (
            ROOT / "pkg/homebrew/Formula/goldeneye-tool.rb.tmpl",
            nuspec,
            ROOT / "pkg/chocolatey/tools/chocolateyinstall.ps1.tmpl",
            ROOT / "pkg/nix/goldeneye-tool-bin.nix.tmpl",
            ROOT / "pkg/nix/flake.nix.tmpl",
        ):
            text = template.read_text(encoding="utf-8")
            self.assertIn("{{VERSION}}", text, template)

    def test_renderer_requires_all_six_checksums(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_value:
            temporary = Path(temporary_value)
            checksums = temporary / "checksums.txt"
            lines = []
            expected_hashes = []
            for platform, arch, extension in ASSETS:
                name = f"goldeneye-{platform}-{arch}.{extension}"
                digest = hashlib.sha256(name.encode()).hexdigest()
                expected_hashes.append(digest)
                lines.append(f"{digest}  {name}")
            checksums.write_text("\n".join(lines) + "\n", encoding="utf-8")
            output = temporary / "rendered"
            render("v1.2.3", checksums, output)

            rendered_files = tuple(path for path in output.rglob("*") if path.is_file())
            self.assertGreaterEqual(len(rendered_files), 8)
            combined = "\n".join(path.read_text(encoding="utf-8") for path in rendered_files)
            self.assertNotRegex(combined, r"\{\{[A-Z0-9_]+\}\}")
            self.assertIn("v1.2.3", combined)
            for digest in expected_hashes:
                self.assertIn(digest, combined)

            checksums.write_text("\n".join(lines[:-1]) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "no entry"):
                render("1.2.3", checksums, temporary / "incomplete")

    def test_release_verifier_smokes_all_six_archive_shapes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_value:
            artifacts = Path(temporary_value)
            lines = []
            for platform, arch, extension in ASSETS:
                name = f"goldeneye-{platform}-{arch}.{extension}"
                archive = artifacts / name
                executable = "goldeneye.exe" if platform == "windows" else "goldeneye"
                payloads = {
                    executable: b"native-binary-placeholder",
                    "LICENSE": (ROOT / "LICENSE").read_bytes(),
                    "NOTICE": (ROOT / "NOTICE").read_bytes(),
                }
                if extension == "zip":
                    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as bundle:
                        for entry, content in payloads.items():
                            bundle.writestr(entry, content)
                else:
                    with tarfile.open(archive, "w:gz") as bundle:
                        for entry, content in payloads.items():
                            info = tarfile.TarInfo(entry)
                            info.size = len(content)
                            info.mode = 0o755 if entry == executable else 0o644
                            bundle.addfile(info, io.BytesIO(content))
                lines.append(f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {name}")
            checksums = artifacts / "checksums.txt"
            checksums.write_text("\n".join(lines) + "\n", encoding="utf-8")
            verify("1.2.3", checksums, artifacts)

    def test_wrappers_make_checksums_mandatory(self) -> None:
        wrappers = (
            ROOT / "pkg/npm/install.js",
            ROOT / "pkg/pypi/src/goldeneye_tool/_cli.py",
            ROOT / "pkg/go/cmd/goldeneye/main.go",
        )
        for wrapper in wrappers:
            text = wrapper.read_text(encoding="utf-8")
            self.assertIn("checksums.txt has no entry", text, wrapper)
            self.assertIn("checksum mismatch", text, wrapper)
            self.assertIn("https", text.lower(), wrapper)

    def test_release_workflow_declares_native_six_target_matrix(self) -> None:
        workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        for target in (
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
        ):
            self.assertIn(target, workflow)
        for runner in ("ubuntu-24.04", "ubuntu-24.04-arm", "macos-15-intel", "macos-14", "windows-2025", "windows-11-arm"):
            self.assertIn(runner, workflow)
        self.assertIn("packaging/verify_release.py", workflow)
        self.assertIn("publish", workflow)


if __name__ == "__main__":
    unittest.main()
