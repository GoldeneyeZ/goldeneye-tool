# goldeneye-tool

This package installs and launches the native `goldeneye` Rust binary. The
installer selects the release for the current OS and CPU, downloads the
versioned `checksums.txt`, and refuses to install an archive whose SHA-256 does
not match.

```sh
npx goldeneye-tool --version
```

Supported targets are Linux, macOS, and Windows on x64 and arm64. Set
`GOLDENEYE_SKIP_INSTALL=1` only when preparing package metadata without running
the binary.
