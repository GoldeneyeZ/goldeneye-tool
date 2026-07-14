# Nix packages

The repository-root `flake.nix` builds Goldeneye from the checked-out Rust
source. For binary releases, `goldeneye-tool-bin.nix.tmpl` and `flake.nix.tmpl`
are rendered with immutable release URLs and SRI SHA-256 hashes by
`packaging/render_release.py`.
