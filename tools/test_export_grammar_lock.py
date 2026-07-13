from __future__ import annotations

import hashlib
import subprocess
import tempfile
from pathlib import Path
import unittest

from tools.export_grammar_lock import (
    ExportError,
    GitSnapshot,
    git_environment,
    parse_direct_parser,
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


if __name__ == "__main__":
    unittest.main()
