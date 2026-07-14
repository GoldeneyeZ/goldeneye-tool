from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).parents[1] / "src"))

from goldeneye_tool import _cli  # noqa: E402


class LauncherMetadataTests(unittest.TestCase):
    def test_resolves_all_supported_target_shapes(self) -> None:
        self.assertEqual(_cli.platform_spec("Linux", "x86_64"), ("linux", "x64", "tar.gz", "goldeneye"))
        self.assertEqual(_cli.platform_spec("Darwin", "arm64")[1], "arm64")
        self.assertEqual(_cli.platform_spec("Windows", "AMD64"), ("windows", "x64", "zip", "goldeneye.exe"))
        self.assertEqual(_cli.platform_spec("Windows", "aarch64")[1], "arm64")
        with self.assertRaises(RuntimeError):
            _cli.platform_spec("Linux", "i686")

    def test_release_asset_is_versioned_https(self) -> None:
        asset = _cli.release_asset("v1.2.3", "Darwin", "arm64")
        self.assertEqual(asset.name, "goldeneye-darwin-arm64.tar.gz")
        self.assertTrue(asset.url.endswith("/v1.2.3/goldeneye-darwin-arm64.tar.gz"))
        self.assertTrue(asset.checksums_url.endswith("/v1.2.3/checksums.txt"))
        with self.assertRaises(ValueError):
            _cli.release_asset("latest", "Linux", "x86_64")
        with self.assertRaises(ValueError):
            _cli.release_asset("1.2.3", "Linux", "x86_64", "http://example.test")

    def test_checksum_entry_is_exact_and_mandatory(self) -> None:
        digest = "b" * 64
        self.assertEqual(_cli.parse_checksums(f"{digest}  goldeneye-linux-x64.tar.gz\n", "goldeneye-linux-x64.tar.gz"), digest)
        with self.assertRaises(ValueError):
            _cli.parse_checksums(f"{digest}  another.tar.gz", "goldeneye-linux-x64.tar.gz")
        with self.assertRaises(ValueError):
            _cli.parse_checksums("not a checksum", "goldeneye-linux-x64.tar.gz")

    def test_archive_paths_cannot_escape(self) -> None:
        _cli._safe_archive_name("docs/NOTICE")
        with self.assertRaises(RuntimeError):
            _cli._safe_archive_name("../goldeneye")
        with self.assertRaises(RuntimeError):
            _cli._safe_archive_name("C:\\temp\\goldeneye.exe")


if __name__ == "__main__":
    unittest.main()
