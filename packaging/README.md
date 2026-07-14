# Release packaging contract

All package managers consume the same native release payloads:

| OS | CPU | Asset |
| --- | --- | --- |
| Linux | x64 | `goldeneye-linux-x64.tar.gz` |
| Linux | arm64 | `goldeneye-linux-arm64.tar.gz` |
| macOS | x64 | `goldeneye-darwin-x64.tar.gz` |
| macOS | arm64 | `goldeneye-darwin-arm64.tar.gz` |
| Windows | x64 | `goldeneye-windows-x64.zip` |
| Windows | arm64 | `goldeneye-windows-arm64.zip` |

Every archive contains the Rust binary (`goldeneye` or `goldeneye.exe`),
`LICENSE`, and `NOTICE` at its root. Releases also contain `checksums.txt` with
one SHA-256 entry per archive. Downloading shims treat a missing, malformed, or
mismatched checksum as a hard failure.

`render_release.py` converts a version and checksum manifest into publishable
Homebrew, Chocolatey, and binary Nix metadata. It performs no network or
publishing operations.
