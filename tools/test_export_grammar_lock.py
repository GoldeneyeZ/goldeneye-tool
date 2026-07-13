from __future__ import annotations

import subprocess
import tempfile
from pathlib import Path
import unittest

from tools.export_grammar_lock import ExportError, GitSnapshot


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


if __name__ == "__main__":
    unittest.main()
