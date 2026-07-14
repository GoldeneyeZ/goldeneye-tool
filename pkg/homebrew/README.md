# Homebrew formula

`Formula/goldeneye-tool.rb.tmpl` is rendered from a release version and its
`checksums.txt` by `packaging/render_release.py`. The rendered formula pins the
four Unix release archives by SHA-256 and installs `goldeneye`, `LICENSE`, and
`NOTICE`.
