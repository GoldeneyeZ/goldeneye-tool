package main

import (
	"strings"
	"testing"
)

func TestResolveAssetTargets(t *testing.T) {
	t.Parallel()
	tests := []struct {
		goos, goarch, name, executable string
	}{
		{"linux", "amd64", "goldeneye-linux-x64.tar.gz", "goldeneye"},
		{"linux", "arm64", "goldeneye-linux-arm64.tar.gz", "goldeneye"},
		{"darwin", "amd64", "goldeneye-darwin-x64.tar.gz", "goldeneye"},
		{"darwin", "arm64", "goldeneye-darwin-arm64.tar.gz", "goldeneye"},
		{"windows", "amd64", "goldeneye-windows-x64.zip", "goldeneye.exe"},
		{"windows", "arm64", "goldeneye-windows-arm64.zip", "goldeneye.exe"},
	}
	for _, test := range tests {
		spec, err := resolveAsset("v1.2.3", test.goos, test.goarch, defaultReleaseBase)
		if err != nil {
			t.Fatalf("resolveAsset(%s/%s): %v", test.goos, test.goarch, err)
		}
		if spec.name != test.name || spec.executable != test.executable {
			t.Fatalf("resolveAsset(%s/%s) = %s/%s", test.goos, test.goarch, spec.name, spec.executable)
		}
		if !strings.Contains(spec.url, "/v1.2.3/") {
			t.Fatalf("asset URL is not versioned: %s", spec.url)
		}
	}
	if _, err := resolveAsset("1.2.3", "linux", "386", defaultReleaseBase); err == nil {
		t.Fatal("expected unsupported architecture error")
	}
	if _, err := resolveAsset("1.2.3", "linux", "amd64", "http://example.test"); err == nil {
		t.Fatal("expected non-HTTPS URL error")
	}
}

func TestParseChecksumsRequiresExactEntry(t *testing.T) {
	t.Parallel()
	digest := strings.Repeat("c", 64)
	actual, err := parseChecksums(digest+"  goldeneye-linux-x64.tar.gz\n", "goldeneye-linux-x64.tar.gz")
	if err != nil || actual != digest {
		t.Fatalf("parseChecksums() = %q, %v", actual, err)
	}
	if _, err := parseChecksums(digest+"  other.tar.gz\n", "goldeneye-linux-x64.tar.gz"); err == nil {
		t.Fatal("expected missing checksum error")
	}
	if _, err := parseChecksums("bad line", "goldeneye-linux-x64.tar.gz"); err == nil {
		t.Fatal("expected malformed checksum error")
	}
}

func TestSafeDestinationRejectsTraversal(t *testing.T) {
	t.Parallel()
	root := t.TempDir()
	if _, err := safeDestination(root, "docs/NOTICE"); err != nil {
		t.Fatalf("safe path rejected: %v", err)
	}
	for _, path := range []string{"../goldeneye", "folder/../../goldeneye", "/tmp/goldeneye"} {
		if _, err := safeDestination(root, path); err == nil {
			t.Fatalf("unsafe path accepted: %s", path)
		}
	}
}
