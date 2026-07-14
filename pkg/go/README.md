# Go install shim

The Go command downloads, verifies, caches, and runs the native Goldeneye Rust
binary. Because the module lives at the repository root, normal release tags
work with `go install`:

```sh
go install github.com/GoldeneyeZ/goldeneye-tool/pkg/go/cmd/goldeneye@v0.1.0
goldeneye --version
```

The module version selects a matching GitHub release. The shim requires the
archive to have an exact SHA-256 entry in that release's `checksums.txt`.
