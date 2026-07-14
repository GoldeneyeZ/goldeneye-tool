"""Resolve, verify, cache, and execute a Goldeneye release binary."""

from __future__ import annotations

import hashlib
import os
from pathlib import Path, PurePosixPath
import platform as host_platform
import re
import shutil
import stat
import sys
import tarfile
import tempfile
from typing import NamedTuple, Optional
from urllib.parse import urljoin, urlparse
from urllib.request import Request, urlopen
import zipfile

try:
    from importlib.metadata import PackageNotFoundError, version as package_version
except ImportError:  # pragma: no cover - Python 3.9 always has importlib.metadata
    PackageNotFoundError = Exception  # type: ignore[assignment]


DEFAULT_VERSION = "0.1.0"
DEFAULT_RELEASE_BASE = "https://github.com/GoldeneyeZ/goldeneye-tool/releases/download"


class ReleaseAsset(NamedTuple):
    version: str
    platform: str
    arch: str
    extension: str
    executable: str
    name: str
    url: str
    checksums_url: str


def normalize_version(value: str) -> str:
    version = value.removeprefix("v")
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?", version):
        raise ValueError(f"invalid release version: {value}")
    return version


def platform_spec(system: Optional[str] = None, machine: Optional[str] = None) -> tuple[str, str, str, str]:
    system_value = (system or host_platform.system()).lower()
    machine_value = (machine or host_platform.machine()).lower()
    systems = {"darwin": "darwin", "linux": "linux", "windows": "windows"}
    arches = {"x86_64": "x64", "amd64": "x64", "aarch64": "arm64", "arm64": "arm64"}
    release_platform = systems.get(system_value)
    release_arch = arches.get(machine_value)
    if not release_platform or not release_arch:
        raise RuntimeError(f"unsupported platform: {system_value}/{machine_value}")
    if release_platform == "windows":
        return release_platform, release_arch, "zip", "goldeneye.exe"
    return release_platform, release_arch, "tar.gz", "goldeneye"


def release_asset(
    version_value: str,
    system: Optional[str] = None,
    machine: Optional[str] = None,
    base_value: str = DEFAULT_RELEASE_BASE,
) -> ReleaseAsset:
    version = normalize_version(version_value)
    base = base_value.rstrip("/")
    if urlparse(base).scheme != "https":
        raise ValueError("release base URL must use HTTPS")
    release_platform, arch, extension, executable = platform_spec(system, machine)
    name = f"goldeneye-{release_platform}-{arch}.{extension}"
    tag_base = f"{base}/v{version}/"
    return ReleaseAsset(
        version,
        release_platform,
        arch,
        extension,
        executable,
        name,
        urljoin(tag_base, name),
        urljoin(tag_base, "checksums.txt"),
    )


def parse_checksums(text: str, asset_name: str) -> str:
    found: Optional[str] = None
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        match = re.fullmatch(r"([0-9a-fA-F]{64})\s+\*?(.+)", line)
        if not match:
            raise ValueError(f"malformed checksum line: {line}")
        if Path(match.group(2).strip()).name == asset_name:
            if found is not None:
                raise ValueError(f"duplicate checksum for {asset_name}")
            found = match.group(1).lower()
    if found is None:
        raise ValueError(f"checksums.txt has no entry for {asset_name}")
    return found


def _download(url: str, destination: Path, user_agent_version: str) -> None:
    if urlparse(url).scheme != "https":
        raise RuntimeError(f"refusing non-HTTPS download: {url}")
    request = Request(url, headers={"User-Agent": f"goldeneye-tool-pypi/{user_agent_version}"})
    with urlopen(request, timeout=60) as response:  # noqa: S310 - scheme checked above
        final_url = response.geturl()
        if urlparse(final_url).scheme != "https":
            raise RuntimeError(f"refusing redirect to non-HTTPS URL: {final_url}")
        if response.status != 200:
            raise RuntimeError(f"download failed with HTTP {response.status}: {url}")
        with destination.open("xb") as output:
            shutil.copyfileobj(response, output)


def _verify_archive(archive: Path, expected: str) -> None:
    digest = hashlib.sha256()
    with archive.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    actual = digest.hexdigest()
    if actual != expected:
        raise RuntimeError(f"checksum mismatch for {archive.name}: expected {expected}, got {actual}")


def _safe_archive_name(name: str) -> None:
    normalized = name.replace("\\", "/")
    path = PurePosixPath(normalized)
    if path.is_absolute() or ".." in path.parts or re.match(r"^[A-Za-z]:", normalized):
        raise RuntimeError(f"unsafe archive entry: {name}")


def _extract_archive(archive: Path, destination: Path, extension: str) -> None:
    if extension == "tar.gz":
        with tarfile.open(archive, "r:gz") as bundle:
            for member in bundle.getmembers():
                _safe_archive_name(member.name)
                if member.issym() or member.islnk():
                    raise RuntimeError(f"archive links are not allowed: {member.name}")
            bundle.extractall(destination)  # noqa: S202 - members validated above
        return
    with zipfile.ZipFile(archive) as bundle:
        for member in bundle.infolist():
            _safe_archive_name(member.filename)
        bundle.extractall(destination)


def installed_version() -> str:
    override = os.environ.get("GOLDENEYE_VERSION")
    if override:
        return normalize_version(override)
    try:
        return normalize_version(package_version("goldeneye-tool"))
    except PackageNotFoundError:
        return DEFAULT_VERSION


def _cache_root() -> Path:
    override = os.environ.get("GOLDENEYE_CACHE_DIR")
    if override:
        return Path(override)
    if sys.platform == "win32":
        return Path(os.environ.get("LOCALAPPDATA", Path.home() / "AppData" / "Local")) / "Goldeneye"
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Caches" / "goldeneye"
    return Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "goldeneye"


def ensure_binary() -> Path:
    version = installed_version()
    asset = release_asset(
        version,
        base_value=os.environ.get("GOLDENEYE_RELEASE_BASE_URL", DEFAULT_RELEASE_BASE),
    )
    binary = _cache_root() / "releases" / version / f"{asset.platform}-{asset.arch}" / asset.executable
    if binary.is_file():
        return binary
    binary.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="goldeneye-install-") as temporary_value:
        temporary = Path(temporary_value)
        archive = temporary / asset.name
        checksums = temporary / "checksums.txt"
        _download(asset.checksums_url, checksums, version)
        _download(asset.url, archive, version)
        expected = parse_checksums(checksums.read_text(encoding="utf-8"), asset.name)
        _verify_archive(archive, expected)
        unpacked = temporary / "unpacked"
        unpacked.mkdir()
        _extract_archive(archive, unpacked, asset.extension)
        source = unpacked / asset.executable
        if not source.is_file():
            raise RuntimeError(f"archive does not contain {asset.executable}")
        staged = binary.with_suffix(binary.suffix + ".tmp")
        shutil.copy2(source, staged)
        if sys.platform != "win32":
            staged.chmod(staged.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
        os.replace(staged, binary)
    return binary


def main() -> None:
    try:
        binary = ensure_binary()
    except Exception as error:  # launcher must present a compact actionable failure
        print(f"goldeneye: installation failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
    os.execv(binary, [str(binary), *sys.argv[1:]])
