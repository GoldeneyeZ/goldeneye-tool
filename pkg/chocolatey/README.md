# Chocolatey package

The nuspec and install script are rendered by `packaging/render_release.py`.
The install script detects Windows x64 or arm64, downloads the matching
versioned ZIP, and delegates SHA-256 verification to Chocolatey before
extraction. Chocolatey then exposes the bundled `goldeneye.exe` as a command.
