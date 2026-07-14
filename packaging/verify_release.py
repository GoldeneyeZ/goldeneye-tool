#!/usr/bin/env python3
"""Verify release archives and checksums without uploading anything."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path, PurePosixPath
import re
import tarfile
import zipfile

from render_release import ASSETS, normalize_version, parse_checksums


def safe_name(name: str) -> bool:
    normalized = name.replace("\\", "/")
    path = PurePosixPath(normalized)
    return not path.is_absolute() and ".." not in path.parts and re.match(r"^[A-Za-z]:", normalized) is None


def archive_entries(path: Path) -> set[str]:
    if path.suffix == ".zip":
        with zipfile.ZipFile(path) as archive:
            names = {entry.filename.rstrip("/") for entry in archive.infolist() if not entry.is_dir()}
    else:
        with tarfile.open(path, "r:gz") as archive:
            members = archive.getmembers()
            if any(member.issym() or member.islnk() for member in members):
                raise ValueError(f"archive links are forbidden: {path.name}")
            names = {member.name.rstrip("/") for member in members if member.isfile()}
    if any(not safe_name(name) for name in names):
        raise ValueError(f"unsafe path in {path.name}")
    return names


def verify(version_value: str, checksums_path: Path, artifacts: Path) -> None:
    normalize_version(version_value)
    checksums = parse_checksums(checksums_path)
    for platform, arch, extension in ASSETS:
        name = f"goldeneye-{platform}-{arch}.{extension}"
        path = artifacts / name
        if not path.is_file():
            raise ValueError(f"missing release asset: {name}")
        expected = checksums.get(name)
        if expected is None:
            raise ValueError(f"checksums.txt has no entry for {name}")
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != expected:
            raise ValueError(f"checksum mismatch for {name}: expected {expected}, got {actual}")
        executable = "goldeneye.exe" if platform == "windows" else "goldeneye"
        required = {executable, "LICENSE", "NOTICE"}
        missing = required - archive_entries(path)
        if missing:
            raise ValueError(f"{name} is missing: {', '.join(sorted(missing))}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--checksums", required=True, type=Path)
    parser.add_argument("--artifacts", required=True, type=Path)
    arguments = parser.parse_args()
    verify(arguments.version, arguments.checksums.resolve(), arguments.artifacts.resolve())


if __name__ == "__main__":
    main()
