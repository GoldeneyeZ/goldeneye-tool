# goldeneye-tool

The Python package is a small launcher for the native `goldeneye` Rust binary.
On first use it downloads the release matching the package version and current
platform, verifies the archive against the release `checksums.txt`, caches the
binary, and replaces the launcher process with it.

```sh
pip install goldeneye-tool
goldeneye --version
```

Linux, macOS, and Windows are supported on x64 and arm64.
