#!/usr/bin/env python3
"""Export Goldeneye's deterministic full Tree-sitter grammar-pack lock.

This script is intentionally Python-stdlib-only and reads an already-present,
pinned codebase-memory-mcp checkout. It never fetches from the network.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import struct
import subprocess
import sys
import tempfile
from typing import Iterable, Iterator


EXPECTED_GRAMMARS = 159
EXPECTED_BINDINGS = 160
EXPECTED_ASSETS = 907
EXPECTED_ABI = {13: 9, 14: 78, 15: 72}
EXPECTED_ORPHANS = {"objectscript_routine", "objectscript_udl"}
EXPECTED_CORE_HASHES = {
    "ada": "fe745430ec54b5c325ce94f94473855fdedde38d9f98e4cd01d5431ef438ff0e",
    "yaml": "cf48df798c4e0c179a91408c9190d7dd6ab8b3736df09626f7c42f977b421a95",
    "rst": "c676d0843e42f086ceda2d889c41cb83eccc67e97f11589a1175a72270bb9da7",
}
ASSET_HASH_DOMAIN = b"goldeneye-grammar-assets-v1\0"
COPY_BUFFER = 1024 * 1024
ALLOWED_SOURCE_SUFFIXES = {".c", ".h", ".inc"}
UPSTREAM_REPOSITORY = "https://github.com/DeusData/codebase-memory-mcp"


class ExportError(RuntimeError):
    """A deterministic export invariant failed."""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--expected-commit", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    return parser.parse_args()


def fail(message: str) -> "None":
    raise ExportError(message)


def is_reparse(metadata: os.stat_result) -> bool:
    attributes = getattr(metadata, "st_file_attributes", 0)
    reparse_flag = getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400)
    return stat.S_ISLNK(metadata.st_mode) or bool(attributes & reparse_flag)


def canonical_safe_directory(path: Path) -> Path:
    absolute = Path(os.path.abspath(path))
    chain = list(reversed((absolute, *absolute.parents)))
    for component in chain:
        if component == component.parent:
            continue
        try:
            metadata = component.lstat()
        except FileNotFoundError as error:
            raise ExportError(f"missing path component: {component}") from error
        if is_reparse(metadata):
            fail(f"symlink/reparse path component rejected: {component}")
    resolved = absolute.resolve(strict=True)
    metadata = resolved.lstat()
    if is_reparse(metadata) or not stat.S_ISDIR(metadata.st_mode):
        fail(f"not a regular directory: {resolved}")
    return resolved


def git_environment() -> dict[str, str]:
    environment = os.environ.copy()
    environment["GIT_NO_REPLACE_OBJECTS"] = "1"
    environment["GIT_NO_LAZY_FETCH"] = "1"
    return environment


def run_git(source: Path, *arguments: str) -> str:
    process = subprocess.run(
        ["git", "-C", str(source), *arguments],
        check=False,
        env=git_environment(),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    if process.returncode != 0:
        fail(f"git {' '.join(arguments)} failed: {process.stderr.strip()}")
    return process.stdout.strip()


@dataclass(frozen=True)
class GitEntry:
    mode: str
    object_id: str
    size: int


class GitSnapshot:
    """Read regular blobs from one immutable Git commit without worktree paths."""

    def __init__(self, source: Path, commit: str) -> None:
        self.source = canonical_safe_directory(source)
        resolved = run_git(self.source, "rev-parse", "--verify", f"{commit}^{{commit}}")
        if resolved != commit:
            fail(f"expected exact commit {commit}, resolved {resolved}")
        process = subprocess.run(
            ["git", "-C", str(self.source), "ls-tree", "-r", "-z", "--long", commit],
            check=False,
            env=git_environment(),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        if process.returncode != 0:
            fail(f"git ls-tree failed: {process.stderr.decode('utf-8', 'replace').strip()}")
        self.entries: dict[str, GitEntry] = {}
        for raw_record in process.stdout.split(b"\0"):
            if not raw_record:
                continue
            try:
                raw_metadata, raw_path = raw_record.split(b"\t", 1)
                mode, object_type, raw_object_id, raw_size = raw_metadata.split()
                path = raw_path.decode("utf-8")
                object_id = raw_object_id.decode("ascii")
                size = int(raw_size)
            except (UnicodeDecodeError, ValueError) as error:
                raise ExportError("invalid UTF-8 or metadata in pinned Git tree") from error
            if object_type != b"blob":
                fail(f"non-blob Git entry rejected: {path}")
            if path in self.entries:
                fail(f"duplicate path in pinned Git tree: {path}")
            self.entries[path] = GitEntry(mode.decode("ascii"), object_id, size)
        self._batch: subprocess.Popen[bytes] | None = None

    def __enter__(self) -> "GitSnapshot":
        return self

    def __exit__(self, _type: object, _value: object, _traceback: object) -> None:
        self.close()

    def close(self) -> None:
        if self._batch is None:
            return
        assert self._batch.stdin is not None
        assert self._batch.stdout is not None
        assert self._batch.stderr is not None
        self._batch.stdin.close()
        self._batch.stdout.close()
        stderr = self._batch.stderr.read()
        self._batch.stderr.close()
        returncode = self._batch.wait()
        self._batch = None
        if returncode != 0:
            fail(f"git cat-file failed: {stderr.decode('utf-8', 'replace').strip()}")

    def paths_under(self, prefix: str) -> list[str]:
        normalized = prefix.rstrip("/") + "/"
        return sorted(
            (path for path in self.entries if path.startswith(normalized)),
            key=lambda path: path.encode("utf-8"),
        )

    def entry(self, path: str) -> GitEntry:
        try:
            entry = self.entries[path]
        except KeyError as error:
            raise ExportError(f"missing pinned Git path: {path}") from error
        if entry.mode not in {"100644", "100755"}:
            fail(f"path is not a regular Git blob: {path} (mode {entry.mode})")
        return entry

    def chunks(self, path: str) -> Iterator[bytes]:
        entry = self.entry(path)
        if self._batch is None:
            self._batch = subprocess.Popen(
                ["git", "-C", str(self.source), "cat-file", "--batch"],
                env=git_environment(),
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                bufsize=0,
            )
        assert self._batch.stdin is not None
        assert self._batch.stdout is not None
        self._batch.stdin.write(entry.object_id.encode("ascii") + b"\n")
        self._batch.stdin.flush()
        header = self._batch.stdout.readline().rstrip(b"\n").split()
        expected_header = [
            entry.object_id.encode("ascii"),
            b"blob",
            str(entry.size).encode("ascii"),
        ]
        if header != expected_header:
            fail(f"unexpected git cat-file header for {path}: {header!r}")
        remaining = entry.size
        while remaining:
            chunk = self._batch.stdout.read(min(COPY_BUFFER, remaining))
            if not chunk:
                fail(f"truncated git blob while reading {path}")
            remaining -= len(chunk)
            yield chunk
        if self._batch.stdout.read(1) != b"\n":
            fail(f"missing git cat-file delimiter after {path}")

    def read_bytes(self, path: str) -> bytes:
        return b"".join(self.chunks(path))

    def read_text(self, path: str) -> str:
        try:
            return self.read_bytes(path).decode("utf-8")
        except UnicodeDecodeError as error:
            raise ExportError(f"pinned Git blob is not UTF-8: {path}") from error


def markdown_cells(line: str) -> list[str]:
    if not line.startswith("|") or not line.endswith("|"):
        fail(f"invalid manifest table row: {line!r}")
    return [cell.strip() for cell in line[1:-1].split("|")]


def table_rows(lines: list[str], heading: str, width: int) -> list[list[str]]:
    try:
        start = lines.index(heading)
    except ValueError as error:
        raise ExportError(f"missing manifest table header: {heading}") from error
    rows: list[list[str]] = []
    for line in lines[start + 2 :]:
        if not line.startswith("|"):
            break
        cells = markdown_cells(line)
        if len(cells) != width:
            fail(f"manifest row under {heading!r} has {len(cells)} cells")
        rows.append(cells)
    return rows


def repository_slug(value: str) -> str:
    match = re.search(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", value)
    if match is None:
        fail(f"manifest repository is not recognizable: {value!r}")
    return match.group(0)


def github_repository(value: str) -> str:
    return f"https://github.com/{repository_slug(value)}"


def clean_revision(value: str) -> str:
    revision = value.strip().strip("`")
    if not revision:
        fail("empty manifest revision")
    return revision


def parse_manifest(
    text: str, expected_commit: str
) -> tuple[dict[str, dict[str, object]], dict[str, str]]:
    lines = text.splitlines()
    records: dict[str, dict[str, object]] = {}

    verified_header = (
        "| grammar | cur ABI | upstream repo | pinned commit | verdict | LICENSE |"
    )
    for name, _abi, repository, revision, verdict, _license in table_rows(
        lines, verified_header, 6
    ):
        records[name] = {
            "repository": github_repository(repository),
            "commit": clean_revision(revision),
            "verdict": verdict,
            "provenance_notes": [],
        }

    in_house_header = "| grammar | cur ABI | LICENSE |"
    for name, _abi, _license in table_rows(lines, in_house_header, 3):
        records[name] = {
            "repository": UPSTREAM_REPOSITORY,
            "commit": expected_commit,
            "verdict": "authored-in-house",
            "provenance_notes": [],
        }

    fork_header = "| grammar | cur ABI | original upstream | license |"
    for name, _abi, repository, _license in table_rows(lines, fork_header, 4):
        records[name] = {
            "repository": github_repository(repository),
            "missing_commit_reason": (
                "MANIFEST.md records the original upstream but no pinned revision; "
                f"vendored bytes are pinned by codebase-memory-mcp {expected_commit}"
            ),
            "verdict": "self-maintained-fork",
            "provenance_notes": [],
        }

    disagreement_header = (
        "| grammar | canonical source (decided) | license (verified) | "
        "nvim-treesitter | Helix |"
    )
    for name, repository, _license, _nvim, _helix in table_rows(
        lines, disagreement_header, 5
    ):
        records[name] = {
            "repository": github_repository(repository),
            "missing_commit_reason": (
                "MANIFEST.md records the resolved canonical source but no pinned revision; "
                f"vendored bytes are pinned by codebase-memory-mcp {expected_commit}"
            ),
            "verdict": "registry-disagreement-resolved",
            "provenance_notes": [],
        }

    if len(records) != EXPECTED_GRAMMARS:
        fail(f"manifest produced {len(records)} unique grammar records")

    patch_header = "| grammar | location | patch | reason |"
    local_patches: dict[str, str] = {}
    for name, location, patch, reason in table_rows(lines, patch_header, 4):
        local_patches[name] = f"{location}: {patch}; reason: {reason}"
    if set(local_patches) != {"crystal", "rescript", "purescript"}:
        fail(f"unexpected local-patch grammar set: {sorted(local_patches)}")
    for name, note in local_patches.items():
        records[name]["provenance_notes"].append(note)  # type: ignore[union-attr]

    required_manifest_fragments = {
        "objectscript_udl": "repointed to a per-directory `objectscript_common.h`",
        "objectscript_routine": "repointed to a per-directory `objectscript_common.h`",
        "mojo": "no longer resolves in the upstream repository after a force-push",
    }
    for name, fragment in required_manifest_fragments.items():
        if fragment not in text:
            fail(f"missing required {name} provenance note in MANIFEST.md")
        records[name]["provenance_notes"].append(  # type: ignore[union-attr]
            {
                "objectscript_udl": (
                    "scanner.c include repointed from ../../common/scanner.h to the "
                    "locked objectscript_common.h; generated parser/scanner otherwise upstream"
                ),
                "objectscript_routine": (
                    "scanner.c include repointed from ../../common/scanner.h to the "
                    "locked objectscript_common.h; generated parser/scanner otherwise upstream"
                ),
                "mojo": (
                    "Helix-pinned 3d7c53b8038f no longer resolves after an upstream "
                    "force-push; vendored lsh/tree-sitter-mojo revision is 33193a99afe6"
                ),
            }[name]
        )

    return records, local_patches


def safe_component(component: str) -> None:
    if not component or component in {".", ".."}:
        fail(f"unsafe path component: {component!r}")
    if component[-1] in {".", " "}:
        fail(f"trailing dot/space path component: {component!r}")
    if any(ord(character) < 32 or character in ':<>"|?*\\' for character in component):
        fail(f"reserved character in path component: {component!r}")
    base = component.split(".", 1)[0].upper()
    if base in {"CON", "PRN", "AUX", "NUL", "CLOCK$"} or re.fullmatch(
        r"(?:COM|LPT)[1-9]", base
    ):
        fail(f"reserved Windows path component: {component!r}")


def normalized_relative(path: Path) -> str:
    value = path.as_posix()
    try:
        value.encode("utf-8")
    except UnicodeEncodeError as error:
        raise ExportError(f"non-UTF-8 path rejected: {path!r}") from error
    if value.startswith("/") or "\\" in value:
        fail(f"non-relative or non-normalized path: {value!r}")
    for component in value.split("/"):
        safe_component(component)
    return value


def grammar_assets(snapshot: GitSnapshot, grammar_name: str) -> list[Path]:
    safe_component(grammar_name)
    prefix = f"internal/cbm/vendored/grammars/{grammar_name}/"
    assets: list[Path] = []
    for full_path in snapshot.paths_under(prefix):
        snapshot.entry(full_path)
        relative = Path(full_path[len(prefix) :])
        is_license = relative.parent == Path(".") and relative.name == "LICENSE"
        if relative.suffix not in ALLOWED_SOURCE_SUFFIXES and not is_license:
            fail(f"unclassified grammar asset rejected: {full_path}")
        assets.append(relative)

    assets.sort(key=lambda path: normalized_relative(path).encode("utf-8"))
    if not assets:
        fail(f"grammar has no compilation assets: {grammar_name}")
    licenses = [path for path in assets if path.parent == Path(".") and path.name == "LICENSE"]
    if licenses != [Path("LICENSE")]:
        fail(f"grammar {grammar_name} must have exactly one direct LICENSE")
    return assets


def parse_direct_parser(
    chunks: Iterable[bytes], parser_path: str, hasher: "hashlib._Hash"
) -> tuple[int, str, int]:
    abi_pattern = re.compile(rb"#define\s+LANGUAGE_VERSION\s+(\d+)(?!\d)")
    symbol_pattern = re.compile(
        rb"(?:TS_PUBLIC\s+|extern\s+)?const\s+TSLanguage\s*\*\s*"
        rb"(tree_sitter_[A-Za-z0-9_]+)\s*\(\s*void\s*\)\s*\{"
    )
    abi_values: list[int] = []
    symbols: set[str] = set()
    total = 0
    overlap = b""
    chunk_iterator = iter(chunk for chunk in chunks if chunk)
    chunk = next(chunk_iterator, None)
    while chunk is not None:
        next_chunk = next(chunk_iterator, None)
        total += len(chunk)
        hasher.update(chunk)
        window = overlap + chunk
        overlap_len = len(overlap)
        physical_end = len(window)
        search_window = window + (next_chunk[:1] if next_chunk is not None else b"")
        abi_values.extend(
            int(match.group(1))
            for match in abi_pattern.finditer(search_window)
            if overlap_len < match.end(1) <= physical_end
        )
        symbols.update(value.decode("ascii") for value in symbol_pattern.findall(window))
        overlap = window[-1024:]
        chunk = next_chunk
    if len(abi_values) != 1:
        fail(f"direct parser must contain exactly one ABI marker: {parser_path}")
    if len(symbols) != 1:
        fail(f"direct parser must export exactly one grammar symbol: {parser_path}")
    abi = abi_values[0]
    if abi not in EXPECTED_ABI:
        fail(f"unsupported grammar ABI {abi}: {parser_path}")
    return abi, next(iter(symbols)), total


def hash_assets(
    snapshot: GitSnapshot, grammar_name: str, assets: list[Path]
) -> tuple[str, int, str, int]:
    hasher = hashlib.sha256(ASSET_HASH_DOMAIN)
    direct_parser = Path("parser.c")
    abi: int | None = None
    exported_symbol: str | None = None
    total_bytes = 0

    for relative_path in assets:
        relative = normalized_relative(relative_path)
        relative_bytes = relative.encode("utf-8")
        source_path = f"internal/cbm/vendored/grammars/{grammar_name}/{relative}"
        entry = snapshot.entry(source_path)
        hasher.update(struct.pack(">Q", len(relative_bytes)))
        hasher.update(relative_bytes)
        hasher.update(struct.pack(">Q", entry.size))

        if relative_path == direct_parser:
            abi, exported_symbol, copied = parse_direct_parser(
                snapshot.chunks(source_path), source_path, hasher
            )
            if copied != entry.size:
                fail(f"pinned parser size mismatch: {source_path}")
            total_bytes += copied
            continue

        copied = 0
        for chunk in snapshot.chunks(source_path):
            copied += len(chunk)
            hasher.update(chunk)
        if copied != entry.size:
            fail(f"pinned asset size mismatch: {source_path}")
        total_bytes += copied

    if abi is None or exported_symbol is None:
        fail(f"grammar {grammar_name} lacks a direct ABI-bearing parser.c")
    return hasher.hexdigest(), abi, exported_symbol, total_bytes


def parse_wrappers(snapshot: GitSnapshot) -> dict[str, str]:
    wrappers: dict[str, str] = {}
    include_pattern = re.compile(r'vendored/grammars/([^/]+)/parser\.c')
    prefix = "internal/cbm/"
    wrapper_paths = [
        path
        for path in snapshot.paths_under(prefix)
        if "/" not in path[len(prefix) :]
        and path[len(prefix) :].startswith("grammar_")
        and path.endswith(".c")
    ]
    for wrapper_path in wrapper_paths:
        wrapper = wrapper_path[len(prefix) :]
        matches = include_pattern.findall(snapshot.read_text(wrapper_path))
        if len(matches) != 1:
            fail(f"wrapper must include exactly one direct parser: {wrapper}")
        grammar = matches[0]
        if grammar in wrappers:
            fail(f"multiple wrappers target grammar {grammar}")
        wrappers[grammar] = wrapper
    if len(wrappers) != 157:
        fail(f"expected 157 wrapper-bound grammars, found {len(wrappers)}")
    return wrappers


def parse_language_ids(workspace_root: Path, enum_source: str) -> list[str]:
    enum_ids = [
        value.lower()
        for value in re.findall(
            r"^\s*CBM_LANG_([A-Z0-9_]+)\s*(?:=\s*[^,]+)?\s*,?",
            enum_source,
            re.MULTILINE,
        )
        if value != "COUNT"
    ]
    tsv_path = workspace_root / "crates/goldeneye-discovery/data/languages.tsv"
    tsv_lines = tsv_path.read_text(encoding="utf-8-sig").splitlines()
    data_lines = [line for line in tsv_lines if line and not line.startswith("#")]
    if not data_lines or data_lines[0].split("\t")[0] != "id":
        fail(f"invalid generated language registry header: {tsv_path}")
    tsv_ids = [line.split("\t")[0] for line in data_lines[1:]]
    if enum_ids != tsv_ids or len(enum_ids) != EXPECTED_BINDINGS:
        fail("cbm.h language enum and generated languages.tsv do not match exactly")
    return enum_ids


def parse_language_factories(source: str) -> dict[str, str]:
    start = source.index("static const CBMLangSpec lang_specs")
    end = source.index("_Static_assert", start)
    table = source[start:end]
    marker = re.compile(r"//\s*CBM_LANG_([A-Z0-9_]+)")
    matches = list(marker.finditer(table))
    factories: dict[str, str] = {}
    for index, match in enumerate(matches):
        block_end = matches[index + 1].start() if index + 1 < len(matches) else len(table)
        block = table[match.start() : block_end]
        factory_matches = re.findall(r"\b(tree_sitter_[A-Za-z0-9_]+)\b", block)
        if not factory_matches:
            fail(f"language spec {match.group(1)} has no Tree-sitter factory")
        language_id = match.group(1).lower()
        if language_id in factories:
            fail(f"duplicate language spec for {language_id}")
        factories[language_id] = factory_matches[-1]
    if len(factories) != 159 or "nim" in factories:
        fail("language spec table must contain 159 entries and omit only Nim")
    return factories


def toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def toml_array(values: Iterable[str]) -> str:
    return "[" + ", ".join(toml_string(value) for value in values) + "]"


def emit_lock(
    expected_commit: str,
    grammar_records: list[dict[str, object]],
    language_mappings: list[dict[str, str]],
) -> str:
    lines = [
        "# Generated by tools/export_grammar_lock.py; do not edit by hand.",
        "schema_version = 1",
        f"upstream_repository = {toml_string(UPSTREAM_REPOSITORY)}",
        f"upstream_commit = {toml_string(expected_commit)}",
        f"declared_grammar_count = {len(grammar_records)}",
        f"declared_language_binding_count = {len(language_mappings)}",
        "compatible_abi_min = 13",
        "compatible_abi_max = 15",
        'hash_algorithm = "sha256"',
        'hash_domain = "goldeneye-grammar-assets-v1"',
        "",
    ]
    for record in grammar_records:
        lines.extend(
            [
                "[[grammars]]",
                f"name = {toml_string(str(record['name']))}",
                f"repository = {toml_string(str(record['repository']))}",
            ]
        )
        if "commit" in record:
            lines.append(f"commit = {toml_string(str(record['commit']))}")
        else:
            lines.append(
                "missing_commit_reason = "
                + toml_string(str(record["missing_commit_reason"]))
            )
        lines.extend(
            [
                f"abi = {record['abi']}",
                f"assets = {toml_array(record['assets'])}",  # type: ignore[arg-type]
                f"source_hash = {toml_string(str(record['source_hash']))}",
                f"scanner_language = {toml_string(str(record['scanner_language']))}",
                f"license_files = {toml_array(record['license_files'])}",  # type: ignore[arg-type]
                f"verdict = {toml_string(str(record['verdict']))}",
                f"provenance_notes = {toml_array(record['provenance_notes'])}",  # type: ignore[arg-type]
            ]
        )
        if "orphan_reason" in record:
            lines.append(
                f"orphan_reason = {toml_string(str(record['orphan_reason']))}"
            )
        lines.append("")

    for mapping in language_mappings:
        lines.extend(
            [
                "[[language_mappings]]",
                f"language_id = {toml_string(mapping['language_id'])}",
                f"status = {toml_string(mapping['status'])}",
            ]
        )
        if mapping["status"] == "available":
            lines.append(f"grammar = {toml_string(mapping['grammar'])}")
        else:
            lines.append(f"reason = {toml_string(mapping['reason'])}")
        lines.append("")
    return "\n".join(lines)


def export(source: Path, expected_commit: str, workspace_root: Path) -> str:
    source = canonical_safe_directory(source)
    actual_commit = run_git(source, "rev-parse", "HEAD")
    if actual_commit != expected_commit:
        fail(f"expected upstream commit {expected_commit}, found {actual_commit}")
    with GitSnapshot(source, expected_commit) as snapshot:
        return export_snapshot(snapshot, expected_commit, workspace_root)


def export_snapshot(
    snapshot: GitSnapshot, expected_commit: str, workspace_root: Path
) -> str:
    grammar_root = "internal/cbm/vendored/grammars"
    manifest_records, _patches = parse_manifest(
        snapshot.read_text(f"{grammar_root}/MANIFEST.md"), expected_commit
    )
    grammar_prefix = f"{grammar_root}/"
    grammar_names = sorted(
        {
            remainder.split("/", 1)[0]
            for path in snapshot.paths_under(grammar_root)
            if "/" in (remainder := path[len(grammar_prefix) :])
        },
        key=lambda name: name.encode("utf-8"),
    )
    if len(grammar_names) != EXPECTED_GRAMMARS:
        fail(f"expected {EXPECTED_GRAMMARS} grammar directories, found {len(grammar_names)}")
    if set(grammar_names) != set(manifest_records):
        fail("manifest provenance rows and grammar directories differ")

    wrapper_by_grammar = parse_wrappers(snapshot)
    language_ids = parse_language_ids(
        workspace_root, snapshot.read_text("internal/cbm/cbm.h")
    )
    language_factories = parse_language_factories(
        snapshot.read_text("internal/cbm/lang_specs.c")
    )

    symbol_to_grammar: dict[str, str] = {}
    records: list[dict[str, object]] = []
    abi_histogram: dict[int, int] = {}
    total_assets = 0
    for grammar_name in grammar_names:
        assets = grammar_assets(snapshot, grammar_name)
        source_hash, abi, symbol, _total_bytes = hash_assets(
            snapshot, grammar_name, assets
        )
        if symbol in symbol_to_grammar:
            fail(f"duplicate exported grammar symbol {symbol}")
        symbol_to_grammar[symbol] = grammar_name
        abi_histogram[abi] = abi_histogram.get(abi, 0) + 1
        total_assets += len(assets)

        provenance = manifest_records[grammar_name]
        record: dict[str, object] = {
            "name": grammar_name,
            **provenance,
            "abi": abi,
            "assets": [normalized_relative(path) for path in assets],
            "source_hash": source_hash,
            "scanner_language": (
                "c" if any(path.name == "scanner.c" for path in assets) else "none"
            ),
            "license_files": ["LICENSE"],
        }
        records.append(record)

    if abi_histogram != EXPECTED_ABI:
        fail(f"parser.c ABI histogram mismatch: {abi_histogram}")
    if total_assets != EXPECTED_ASSETS:
        fail(f"expected {EXPECTED_ASSETS} locked assets, found {total_assets}")
    for grammar, expected_hash in EXPECTED_CORE_HASHES.items():
        actual_hash = next(
            record["source_hash"] for record in records if record["name"] == grammar
        )
        if actual_hash != expected_hash:
            fail(f"{grammar} source hash mismatch: {actual_hash}")

    for grammar, wrapper in wrapper_by_grammar.items():
        symbol = next(
            (symbol for symbol, owner in symbol_to_grammar.items() if owner == grammar), None
        )
        if symbol is None:
            fail(f"wrapper {wrapper} targets grammar without an exported symbol: {grammar}")

    mappings: list[dict[str, str]] = []
    for language_id in language_ids:
        if language_id == "nim":
            mappings.append(
                {
                    "language_id": "nim",
                    "status": "unavailable",
                    "reason": (
                        "codebase-memory-mcp retains the language ID but has no lang_specs "
                        "entry or Tree-sitter factory at the pinned commit"
                    ),
                }
            )
            continue
        factory = language_factories.get(language_id)
        if factory is None:
            fail(f"available language {language_id} has no factory")
        grammar = symbol_to_grammar.get(factory)
        if grammar is None:
            fail(f"language factory {factory} does not resolve to a grammar asset")
        if grammar not in wrapper_by_grammar:
            fail(f"language {language_id} resolves to unwrapped grammar {grammar}")
        mappings.append(
            {
                "language_id": language_id,
                "status": "available",
                "grammar": grammar,
            }
        )

    mappings.sort(key=lambda mapping: mapping["language_id"].encode("utf-8"))
    bound = {
        mapping["grammar"] for mapping in mappings if mapping["status"] == "available"
    }
    orphans = {record["name"] for record in records} - bound
    if orphans != EXPECTED_ORPHANS or len(bound) != 157:
        fail(f"unexpected bound/orphan grammar sets: bound={len(bound)}, orphans={orphans}")
    for record in records:
        if record["name"] in orphans:
            record["orphan_reason"] = (
                "vendored ObjectScript asset has no CBMLangSpec language binding at the "
                "pinned upstream commit"
            )

    special = {mapping["language_id"]: mapping.get("grammar") for mapping in mappings}
    if special.get("yaml") != "yaml" or special.get("kustomize") != "yaml" or special.get("k8s") != "yaml":
        fail("YAML/Kustomize/K8s shared-grammar mapping changed")
    if sum(1 for mapping in mappings if mapping["status"] == "available") != 159:
        fail("available language count changed")

    return emit_lock(expected_commit, records, mappings)


def write_atomic(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(
        prefix=f".{path.name}.", suffix=".tmp", dir=path.parent
    )
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as output:
            output.write(content)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    except BaseException:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise


def main() -> int:
    arguments = parse_args()
    workspace_root = Path(__file__).resolve().parent.parent
    try:
        content = export(arguments.source, arguments.expected_commit, workspace_root)
        if arguments.check:
            try:
                existing = arguments.output.read_text(encoding="utf-8")
            except FileNotFoundError:
                fail(f"lock file does not exist: {arguments.output}")
            if existing != content:
                fail(f"lock file is stale: {arguments.output}")
            print(f"grammar lock is reproducible: {arguments.output}")
            return 0
        write_atomic(arguments.output, content)
        print(f"wrote {arguments.output}")
        return 0
    except ExportError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
