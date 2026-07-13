from __future__ import annotations

import hashlib
import re
import subprocess
import sys
import tempfile
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from tools.export_grammar_lock import (
    ExportError,
    GitEntry,
    GitSnapshot,
    emit_lock,
    git_environment,
    hash_assets,
    parse_direct_parser,
    register_exported_symbol,
    resolve_language_mappings,
)


class GitSnapshotTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.repository = Path(self.temporary.name)
        self.git("init", "--quiet")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("config", "user.name", "Fixture")

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def git(self, *arguments: str, input_bytes: bytes | None = None) -> str:
        process = subprocess.run(
            ["git", "-C", str(self.repository), *arguments],
            input=input_bytes,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=True,
        )
        return process.stdout.decode("utf-8").strip()

    def commit(self) -> str:
        self.git("commit", "--quiet", "-m", "fixture")
        return self.git("rev-parse", "HEAD")

    def test_reads_pinned_blob_after_worktree_replacement(self) -> None:
        asset = self.repository / "grammar" / "parser.c"
        asset.parent.mkdir()
        asset.write_bytes(b"committed parser")
        self.git("add", "grammar/parser.c")
        commit = self.commit()
        asset.write_bytes(b"worktree replacement")

        with GitSnapshot(self.repository, commit) as snapshot:
            self.assertEqual(
                snapshot.read_bytes("grammar/parser.c"), b"committed parser"
            )

    def test_rejects_git_symlink_mode_without_opening_worktree(self) -> None:
        blob = self.git("hash-object", "-w", "--stdin", input_bytes=b"outside.c")
        self.git("update-index", "--add", "--cacheinfo", f"120000,{blob},link.c")
        commit = self.commit()

        with GitSnapshot(self.repository, commit) as snapshot:
            with self.assertRaisesRegex(ExportError, "regular Git blob"):
                snapshot.read_bytes("link.c")

    def test_ignores_git_replacement_refs_for_exact_commit_bytes(self) -> None:
        asset = self.repository / "grammar" / "parser.c"
        asset.parent.mkdir()
        asset.write_bytes(b"original parser")
        self.git("add", "grammar/parser.c")
        original = self.commit()

        asset.write_bytes(b"replacement parser")
        self.git("add", "grammar/parser.c")
        replacement = self.commit()
        self.git("replace", original, replacement)

        with GitSnapshot(self.repository, original) as snapshot:
            self.assertEqual(snapshot.read_bytes("grammar/parser.c"), b"original parser")

    def test_disables_replacements_and_lazy_object_fetches(self) -> None:
        environment = git_environment()

        self.assertEqual(environment["GIT_NO_REPLACE_OBJECTS"], "1")
        self.assertEqual(environment["GIT_NO_LAZY_FETCH"], "1")


class DirectParserTests(unittest.TestCase):
    @staticmethod
    def parse(chunks: list[bytes]) -> tuple[int, str, int, str]:
        hasher = hashlib.sha256()
        abi, symbol, total = parse_direct_parser(
            chunks, "fixture/parser.c", hasher
        )
        return abi, symbol, total, hasher.hexdigest()

    def test_rejects_duplicate_identical_abi_markers(self) -> None:
        parser = (
            b"#define LANGUAGE_VERSION 14\n"
            b"#define LANGUAGE_VERSION 14\n"
            b"const TSLanguage *tree_sitter_fixture(void) {\n"
        )

        with self.assertRaisesRegex(ExportError, "exactly one ABI marker"):
            parse_direct_parser([parser], "fixture/parser.c", hashlib.sha256())

    def test_counts_abi_marker_once_at_every_chunk_boundary(self) -> None:
        marker = b"#define LANGUAGE_VERSION 14\n"
        symbol = b"const TSLanguage *tree_sitter_fixture(void) {\n"

        for split in range(1, len(marker)):
            chunks = [marker[:split], marker[split:] + symbol]
            with self.subTest(split=split):
                abi, exported_symbol, total = parse_direct_parser(
                    chunks, "fixture/parser.c", hashlib.sha256()
                )

                self.assertEqual(abi, 14)
                self.assertEqual(exported_symbol, "tree_sitter_fixture")
                self.assertEqual(total, sum(map(len, chunks)))

    def test_accepts_abi_marker_at_eof_without_newline(self) -> None:
        chunks = [
            b"const TSLanguage *tree_sitter_fixture(void) {\n",
            b"#define LANGUAGE_VERSION 14",
        ]

        abi, symbol, total = parse_direct_parser(
            chunks, "fixture/parser.c", hashlib.sha256()
        )

        self.assertEqual(abi, 14)
        self.assertEqual(symbol, "tree_sitter_fixture")
        self.assertEqual(total, sum(map(len, chunks)))

    def test_extracts_cobol_factory_through_every_single_byte_chunk(self) -> None:
        parser = (
            b"#define LANGUAGE_VERSION 15\n"
            b"TS_PUBLIC const TSLanguage *tree_sitter_COBOL(void) {\n"
            b"  return &language;\n"
            b"}"
        )
        chunks = [parser[index : index + 1] for index in range(len(parser))]

        abi, symbol, total, digest = self.parse(chunks)

        self.assertEqual(abi, 15)
        self.assertEqual(symbol, "tree_sitter_COBOL")
        self.assertEqual(total, len(parser))
        self.assertEqual(digest, hashlib.sha256(parser).hexdigest())

    def test_streams_virtual_104_mib_parser_with_bounded_lookahead(self) -> None:
        class GuardedHasher:
            def __init__(self) -> None:
                self.digest = hashlib.sha256(b"seeded-domain-prefix\0")
                self.update_count = 0

            def update(self, chunk: bytes) -> None:
                self.update_count += 1
                self.digest.update(chunk)

        chunk_size = 1024 * 1024
        chunk_count = 104
        head = (
            b"#define LANGUAGE_VERSION 15\n"
            b"const TSLanguage *tree_sitter_fixture(void) {\n"
        )
        first = head + b"x" * (chunk_size - len(head))
        padding = b"x" * chunk_size
        hasher = GuardedHasher()

        def guarded_chunks() -> object:
            for index in range(chunk_count):
                if index >= 2 and hasher.update_count < index - 1:
                    raise AssertionError("parser chunks were consumed eagerly")
                yield first if index == 0 else padding

        abi, symbol, total = parse_direct_parser(
            guarded_chunks(),  # type: ignore[arg-type]
            "fixture/parser.c",
            hasher,  # type: ignore[arg-type]
        )
        expected = hashlib.sha256(b"seeded-domain-prefix\0")
        expected.update(first)
        for _index in range(1, chunk_count):
            expected.update(padding)

        self.assertEqual((abi, symbol), (15, "tree_sitter_fixture"))
        self.assertEqual(total, 104 * 1024 * 1024)
        self.assertEqual(hasher.digest.hexdigest(), expected.hexdigest())

    def test_extracts_factory_split_at_stream_boundary(self) -> None:
        prefix = b"#define LANGUAGE_VERSION 14\nconst TSLanguage *tree_sitter_"
        chunks = [prefix, b"fixture(void) {"]

        abi, symbol, total, digest = self.parse(chunks)

        self.assertEqual(abi, 14)
        self.assertEqual(symbol, "tree_sitter_fixture")
        self.assertEqual(total, sum(map(len, chunks)))
        self.assertEqual(digest, hashlib.sha256(b"".join(chunks)).hexdigest())

    def test_extracts_factory_at_eof_without_final_newline(self) -> None:
        parser = (
            b"#define LANGUAGE_VERSION 13\n"
            b"const TSLanguage *tree_sitter_fixture(void) {"
        )

        abi, symbol, total, _digest = self.parse([parser])

        self.assertEqual((abi, symbol, total), (13, "tree_sitter_fixture", len(parser)))

    def test_ignores_scanner_prototypes_when_extracting_factory(self) -> None:
        parser = (
            b"#define LANGUAGE_VERSION 15\n"
            b"void *tree_sitter_fixture_external_scanner_create(void);\n"
            b"bool tree_sitter_fixture_external_scanner_scan(void *, void *, void *);\n"
            b"const TSLanguage *tree_sitter_fixture(void) {"
        )

        _abi, symbol, _total, _digest = self.parse([parser])

        self.assertEqual(symbol, "tree_sitter_fixture")

    def test_rejects_duplicate_direct_language_factories(self) -> None:
        parser = (
            b"#define LANGUAGE_VERSION 15\n"
            b"const TSLanguage *tree_sitter_fixture(void) { return 0; }\n"
            b"const TSLanguage *tree_sitter_fixture(void) { return 0; }\n"
        )

        with self.assertRaisesRegex(ExportError, "exactly one grammar symbol"):
            self.parse([parser])

    def test_rejects_malformed_non_identifier_factory(self) -> None:
        parser = (
            b"#define LANGUAGE_VERSION 15\n"
            b"const TSLanguage *tree_sitter_bad-name(void) { return 0; }\n"
        )

        with self.assertRaisesRegex(ExportError, "exactly one grammar symbol"):
            self.parse([parser])

    def test_extracts_all_factory_exceptions_and_conventional_names(self) -> None:
        exceptions = {
            "assembly": "tree_sitter_asm",
            "cobol": "tree_sitter_COBOL",
            "gotemplate": "tree_sitter_gotmpl",
            "janet": "tree_sitter_janet_simple",
            "php": "tree_sitter_php_only",
            "protobuf": "tree_sitter_proto",
            "qml": "tree_sitter_qmljs",
            "sshconfig": "tree_sitter_ssh_config",
        }
        workspace = Path(__file__).resolve().parent.parent
        lock_source = (workspace / "grammars/full-pack.toml").read_text(encoding="utf-8")
        grammar_names = re.findall(
            r'^\[\[grammars\]\]\nname = "([A-Za-z0-9_]+)"$',
            lock_source,
            re.MULTILINE,
        )

        self.assertEqual(len(grammar_names), 159)
        self.assertEqual(len(set(grammar_names) - set(exceptions)), 151)
        for grammar in grammar_names:
            expected = exceptions.get(grammar, f"tree_sitter_{grammar}")
            parser = (
                b"#define LANGUAGE_VERSION 15\n"
                + f"const TSLanguage *{expected}(void) {{".encode("ascii")
            )
            with self.subTest(grammar=grammar):
                _abi, symbol, _total, _digest = self.parse([parser])
                self.assertEqual(symbol, expected)


class LockEmissionTests(unittest.TestCase):
    def test_persists_exported_symbol_in_each_grammar_record(self) -> None:
        source = emit_lock(
            "1" * 40,
            [
                {
                    "name": "cobol",
                    "repository": "https://example.invalid/cobol",
                    "commit": "2" * 40,
                    "abi": 15,
                    "assets": ["LICENSE", "parser.c"],
                    "source_hash": "3" * 64,
                    "exported_symbol": "tree_sitter_COBOL",
                    "scanner_language": "none",
                    "license_files": ["LICENSE"],
                    "verdict": "fixture",
                    "provenance_notes": [],
                }
            ],
            [
                {
                    "language_id": "cobol",
                    "status": "available",
                    "grammar": "cobol",
                }
            ],
        )

        self.assertIn('exported_symbol = "tree_sitter_COBOL"', source)


class ExportCrossCheckTests(unittest.TestCase):
    def test_hash_assets_streams_parser_without_read_bytes(self) -> None:
        parser = (
            b"#define LANGUAGE_VERSION 15\n"
            b"const TSLanguage *tree_sitter_fixture(void) {"
        )
        license_bytes = b"fixture license"

        class StreamingSnapshot:
            def entry(self, path: str) -> GitEntry:
                content = parser if path.endswith("/parser.c") else license_bytes
                return GitEntry("100644", "0" * 40, len(content))

            def chunks(self, path: str) -> object:
                content = parser if path.endswith("/parser.c") else license_bytes
                midpoint = len(content) // 2
                return iter((content[:midpoint], content[midpoint:]))

            def read_bytes(self, _path: str) -> bytes:
                raise AssertionError("parser assets must never use read_bytes")

        source_hash, abi, symbol, total = hash_assets(
            StreamingSnapshot(),  # type: ignore[arg-type]
            "fixture",
            [Path("LICENSE"), Path("parser.c")],
        )

        self.assertEqual(abi, 15)
        self.assertEqual(symbol, "tree_sitter_fixture")
        self.assertEqual(total, len(license_bytes) + len(parser))
        self.assertEqual(len(source_hash), 64)

    def test_rejects_bound_factory_that_resolves_to_wrong_grammar(self) -> None:
        with self.assertRaisesRegex(ExportError, "factory mismatch"):
            resolve_language_mappings(
                ["alpha"],
                {"alpha": "tree_sitter_beta"},
                {
                    "tree_sitter_alpha": "alpha",
                    "tree_sitter_beta": "beta",
                },
                {"alpha": "grammar_alpha.c", "beta": "grammar_beta.c"},
            )

    def test_rejects_globally_duplicate_resolved_symbols(self) -> None:
        symbols: dict[str, str] = {}
        register_exported_symbol(symbols, "tree_sitter_alpha", "alpha")

        with self.assertRaisesRegex(ExportError, "duplicate exported grammar symbol"):
            register_exported_symbol(symbols, "tree_sitter_alpha", "beta")


if __name__ == "__main__":
    unittest.main()
