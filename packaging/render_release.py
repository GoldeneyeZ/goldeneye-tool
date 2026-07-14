#!/usr/bin/env python3
"""Render checksum-pinned package-manager metadata without publishing it."""

from __future__ import annotations

import argparse
import base64
import hashlib
from pathlib import Path
import re
import shutil


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
ASSETS = (
    ("linux", "x64", "tar.gz"),
    ("linux", "arm64", "tar.gz"),
    ("darwin", "x64", "tar.gz"),
    ("darwin", "arm64", "tar.gz"),
    ("windows", "x64", "zip"),
    ("windows", "arm64", "zip"),
)
VERSION_PATTERN = re.compile(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?")
PLACEHOLDER_PATTERN = re.compile(r"\{\{[A-Z0-9_]+\}\}")


def normalize_version(value: str) -> str:
    version = value.removeprefix("v")
    if VERSION_PATTERN.fullmatch(version) is None:
        raise ValueError(f"invalid release version: {value}")
    return version


def parse_checksums(path: Path) -> dict[str, str]:
    checksums: dict[str, str] = {}
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line:
            continue
        match = re.fullmatch(r"([0-9a-fA-F]{64})\s+\*?(.+)", line)
        if match is None:
            raise ValueError(f"malformed checksum line: {line}")
        name = Path(match.group(2).strip()).name
        if name in checksums:
            raise ValueError(f"duplicate checksum for {name}")
        checksums[name] = match.group(1).lower()
    return checksums


def replacements(version_value: str, checksum_path: Path) -> dict[str, str]:
    version = normalize_version(version_value)
    checksums = parse_checksums(checksum_path)
    values = {"VERSION": version}
    for platform, arch, extension in ASSETS:
        name = f"goldeneye-{platform}-{arch}.{extension}"
        try:
            digest = checksums[name]
        except KeyError as error:
            raise ValueError(f"checksums.txt has no entry for {name}") from error
        prefix = f"{platform}_{arch}".upper()
        values[f"{prefix}_SHA256"] = digest
        values[f"{prefix}_SRI"] = "sha256-" + base64.b64encode(bytes.fromhex(digest)).decode("ascii")
    return values


def render_template(source: Path, destination: Path, values: dict[str, str]) -> None:
    text = source.read_text(encoding="utf-8")
    for key, value in values.items():
        text = text.replace("{{" + key + "}}", value)
    unresolved = sorted(set(PLACEHOLDER_PATTERN.findall(text)))
    if unresolved:
        raise ValueError(f"unresolved placeholders in {source}: {', '.join(unresolved)}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(text, encoding="utf-8", newline="\n")


def render(version: str, checksum_path: Path, output: Path) -> None:
    values = replacements(version, checksum_path)
    templates = (
        (
            REPOSITORY_ROOT / "pkg/homebrew/Formula/goldeneye-tool.rb.tmpl",
            output / "homebrew/Formula/goldeneye-tool.rb",
        ),
        (
            REPOSITORY_ROOT / "pkg/chocolatey/goldeneye-tool.nuspec.tmpl",
            output / "chocolatey/goldeneye-tool.nuspec",
        ),
        (
            REPOSITORY_ROOT / "pkg/chocolatey/tools/chocolateyinstall.ps1.tmpl",
            output / "chocolatey/tools/chocolateyinstall.ps1",
        ),
        (
            REPOSITORY_ROOT / "pkg/nix/goldeneye-tool-bin.nix.tmpl",
            output / "nix/package.nix",
        ),
        (
            REPOSITORY_ROOT / "pkg/nix/flake.nix.tmpl",
            output / "nix/flake.nix",
        ),
    )
    for source, destination in templates:
        render_template(source, destination, values)

    output.mkdir(parents=True, exist_ok=True)
    shutil.copy2(REPOSITORY_ROOT / "pkg/chocolatey/tools/chocolateyuninstall.ps1", output / "chocolatey/tools")
    shutil.copy2(REPOSITORY_ROOT / "LICENSE", output / "chocolatey/LICENSE")
    shutil.copy2(REPOSITORY_ROOT / "NOTICE", output / "chocolatey/NOTICE")
    (output / "release-metadata.sha256").write_text(
        hashlib.sha256(checksum_path.read_bytes()).hexdigest() + "  checksums.txt\n",
        encoding="utf-8",
        newline="\n",
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--checksums", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    render(arguments.version, arguments.checksums.resolve(), arguments.output.resolve())


if __name__ == "__main__":
    main()
