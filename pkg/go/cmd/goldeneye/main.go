package main

import (
	"archive/tar"
	"archive/zip"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"runtime/debug"
	"strings"
	"time"
)

const (
	defaultVersion     = "0.1.0"
	defaultReleaseBase = "https://github.com/GoldeneyeZ/goldeneye-tool/releases/download"
)

var versionPattern = regexp.MustCompile(`^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$`)

type assetSpec struct {
	version      string
	platform     string
	arch         string
	extension    string
	executable   string
	name         string
	url          string
	checksumsURL string
}

func normalizeVersion(value string) (string, error) {
	value = strings.TrimPrefix(value, "v")
	if !versionPattern.MatchString(value) {
		return "", fmt.Errorf("invalid release version: %s", value)
	}
	return value, nil
}

func installedVersion() (string, error) {
	if override := os.Getenv("GOLDENEYE_VERSION"); override != "" {
		return normalizeVersion(override)
	}
	if info, ok := debug.ReadBuildInfo(); ok && info.Main.Version != "" && info.Main.Version != "(devel)" {
		return normalizeVersion(info.Main.Version)
	}
	return defaultVersion, nil
}

func resolveAsset(versionValue, goos, goarch, baseValue string) (assetSpec, error) {
	version, err := normalizeVersion(versionValue)
	if err != nil {
		return assetSpec{}, err
	}
	platforms := map[string]string{"darwin": "darwin", "linux": "linux", "windows": "windows"}
	arches := map[string]string{"amd64": "x64", "arm64": "arm64"}
	platform, ok := platforms[goos]
	if !ok {
		return assetSpec{}, fmt.Errorf("unsupported platform: %s/%s", goos, goarch)
	}
	arch, ok := arches[goarch]
	if !ok {
		return assetSpec{}, fmt.Errorf("unsupported platform: %s/%s", goos, goarch)
	}
	extension, executable := "tar.gz", "goldeneye"
	if goos == "windows" {
		extension, executable = "zip", "goldeneye.exe"
	}
	base := strings.TrimRight(baseValue, "/")
	parsed, err := url.Parse(base)
	if err != nil || parsed.Scheme != "https" {
		return assetSpec{}, errors.New("release base URL must use HTTPS")
	}
	name := fmt.Sprintf("goldeneye-%s-%s.%s", platform, arch, extension)
	tagBase := fmt.Sprintf("%s/v%s", base, version)
	return assetSpec{
		version: version, platform: platform, arch: arch, extension: extension,
		executable: executable, name: name, url: tagBase + "/" + name,
		checksumsURL: tagBase + "/checksums.txt",
	}, nil
}

func parseChecksums(text, assetName string) (string, error) {
	var found string
	for _, rawLine := range strings.Split(text, "\n") {
		line := strings.TrimSpace(rawLine)
		if line == "" {
			continue
		}
		fields := strings.Fields(line)
		if len(fields) != 2 || len(fields[0]) != 64 {
			return "", fmt.Errorf("malformed checksum line: %s", line)
		}
		if _, err := hex.DecodeString(fields[0]); err != nil {
			return "", fmt.Errorf("malformed checksum line: %s", line)
		}
		name := strings.TrimPrefix(fields[1], "*")
		if filepath.Base(name) == assetName {
			if found != "" {
				return "", fmt.Errorf("duplicate checksum for %s", assetName)
			}
			found = strings.ToLower(fields[0])
		}
	}
	if found == "" {
		return "", fmt.Errorf("checksums.txt has no entry for %s", assetName)
	}
	return found, nil
}

func httpClient() *http.Client {
	return &http.Client{
		Timeout: 60 * time.Second,
		CheckRedirect: func(request *http.Request, via []*http.Request) error {
			if len(via) >= 5 {
				return errors.New("too many redirects")
			}
			if request.URL.Scheme != "https" {
				return fmt.Errorf("refusing redirect to non-HTTPS URL: %s", request.URL)
			}
			return nil
		},
	}
}

func download(client *http.Client, source string, destination string, version string) error {
	parsed, err := url.Parse(source)
	if err != nil || parsed.Scheme != "https" {
		return fmt.Errorf("refusing non-HTTPS download: %s", source)
	}
	request, err := http.NewRequest(http.MethodGet, source, nil)
	if err != nil {
		return err
	}
	request.Header.Set("User-Agent", "goldeneye-tool-go/"+version)
	response, err := client.Do(request)
	if err != nil {
		return err
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return fmt.Errorf("download failed with HTTP %d: %s", response.StatusCode, source)
	}
	output, err := os.OpenFile(destination, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return err
	}
	_, copyErr := io.Copy(output, response.Body)
	closeErr := output.Close()
	if copyErr != nil {
		return copyErr
	}
	return closeErr
}

func verifyArchive(archive, expected string) error {
	file, err := os.Open(archive)
	if err != nil {
		return err
	}
	defer file.Close()
	hash := sha256.New()
	if _, err := io.Copy(hash, file); err != nil {
		return err
	}
	actual := fmt.Sprintf("%x", hash.Sum(nil))
	if actual != expected {
		return fmt.Errorf("checksum mismatch for %s: expected %s, got %s", filepath.Base(archive), expected, actual)
	}
	return nil
}

func safeDestination(root, name string) (string, error) {
	normalizedSlash := strings.ReplaceAll(name, "\\", "/")
	if regexp.MustCompile(`^[A-Za-z]:`).MatchString(normalizedSlash) {
		return "", fmt.Errorf("unsafe archive entry: %s", name)
	}
	normalized := filepath.FromSlash(normalizedSlash)
	cleaned := filepath.Clean(normalized)
	if filepath.IsAbs(cleaned) || cleaned == ".." || strings.HasPrefix(cleaned, ".."+string(filepath.Separator)) || filepath.VolumeName(cleaned) != "" {
		return "", fmt.Errorf("unsafe archive entry: %s", name)
	}
	destination := filepath.Join(root, cleaned)
	relative, err := filepath.Rel(root, destination)
	if err != nil || relative == ".." || strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
		return "", fmt.Errorf("unsafe archive entry: %s", name)
	}
	return destination, nil
}

func extractTarGz(archive, destination string) error {
	file, err := os.Open(archive)
	if err != nil {
		return err
	}
	defer file.Close()
	gzipReader, err := gzip.NewReader(file)
	if err != nil {
		return err
	}
	defer gzipReader.Close()
	reader := tar.NewReader(gzipReader)
	for {
		header, err := reader.Next()
		if errors.Is(err, io.EOF) {
			return nil
		}
		if err != nil {
			return err
		}
		target, err := safeDestination(destination, header.Name)
		if err != nil {
			return err
		}
		switch header.Typeflag {
		case tar.TypeDir:
			if err := os.MkdirAll(target, 0o755); err != nil {
				return err
			}
		case tar.TypeReg:
			if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
				return err
			}
			output, err := os.OpenFile(target, os.O_WRONLY|os.O_CREATE|os.O_EXCL, os.FileMode(header.Mode)&0o777)
			if err != nil {
				return err
			}
			_, copyErr := io.Copy(output, reader)
			closeErr := output.Close()
			if copyErr != nil {
				return copyErr
			}
			if closeErr != nil {
				return closeErr
			}
		default:
			return fmt.Errorf("unsupported archive entry type: %s", header.Name)
		}
	}
}

func extractZip(archive, destination string) error {
	bundle, err := zip.OpenReader(archive)
	if err != nil {
		return err
	}
	defer bundle.Close()
	for _, member := range bundle.File {
		target, err := safeDestination(destination, member.Name)
		if err != nil {
			return err
		}
		if member.FileInfo().IsDir() {
			if err := os.MkdirAll(target, 0o755); err != nil {
				return err
			}
			continue
		}
		if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
			return err
		}
		input, err := member.Open()
		if err != nil {
			return err
		}
		output, err := os.OpenFile(target, os.O_WRONLY|os.O_CREATE|os.O_EXCL, member.Mode())
		if err != nil {
			input.Close()
			return err
		}
		_, copyErr := io.Copy(output, input)
		inputErr := input.Close()
		outputErr := output.Close()
		if copyErr != nil {
			return copyErr
		}
		if inputErr != nil {
			return inputErr
		}
		if outputErr != nil {
			return outputErr
		}
	}
	return nil
}

func ensureBinary(spec assetSpec) (string, error) {
	cache, err := os.UserCacheDir()
	if err != nil {
		return "", err
	}
	if override := os.Getenv("GOLDENEYE_CACHE_DIR"); override != "" {
		cache = override
	}
	directory := filepath.Join(cache, "goldeneye", "releases", spec.version, spec.platform+"-"+spec.arch)
	binary := filepath.Join(directory, spec.executable)
	if info, err := os.Stat(binary); err == nil && info.Mode().IsRegular() {
		return binary, nil
	}
	if err := os.MkdirAll(directory, 0o755); err != nil {
		return "", err
	}
	temporary, err := os.MkdirTemp("", "goldeneye-install-")
	if err != nil {
		return "", err
	}
	defer os.RemoveAll(temporary)
	archive := filepath.Join(temporary, spec.name)
	checksums := filepath.Join(temporary, "checksums.txt")
	client := httpClient()
	if err := download(client, spec.checksumsURL, checksums, spec.version); err != nil {
		return "", err
	}
	if err := download(client, spec.url, archive, spec.version); err != nil {
		return "", err
	}
	manifest, err := os.ReadFile(checksums)
	if err != nil {
		return "", err
	}
	expected, err := parseChecksums(string(manifest), spec.name)
	if err != nil {
		return "", err
	}
	if err := verifyArchive(archive, expected); err != nil {
		return "", err
	}
	unpacked := filepath.Join(temporary, "unpacked")
	if err := os.Mkdir(unpacked, 0o755); err != nil {
		return "", err
	}
	if spec.extension == "zip" {
		err = extractZip(archive, unpacked)
	} else {
		err = extractTarGz(archive, unpacked)
	}
	if err != nil {
		return "", err
	}
	source := filepath.Join(unpacked, spec.executable)
	if info, err := os.Stat(source); err != nil || !info.Mode().IsRegular() {
		return "", fmt.Errorf("archive does not contain %s", spec.executable)
	}
	staged := binary + ".tmp"
	input, err := os.Open(source)
	if err != nil {
		return "", err
	}
	output, err := os.OpenFile(staged, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0o755)
	if err != nil {
		input.Close()
		return "", err
	}
	_, copyErr := io.Copy(output, input)
	inputErr := input.Close()
	outputErr := output.Close()
	if copyErr != nil {
		return "", copyErr
	}
	if inputErr != nil {
		return "", inputErr
	}
	if outputErr != nil {
		return "", outputErr
	}
	if err := os.Rename(staged, binary); err != nil {
		return "", err
	}
	return binary, nil
}

func run() error {
	version, err := installedVersion()
	if err != nil {
		return err
	}
	spec, err := resolveAsset(version, runtime.GOOS, runtime.GOARCH, envOrDefault("GOLDENEYE_RELEASE_BASE_URL", defaultReleaseBase))
	if err != nil {
		return err
	}
	binary, err := ensureBinary(spec)
	if err != nil {
		return err
	}
	command := exec.Command(binary, os.Args[1:]...)
	command.Stdin = os.Stdin
	command.Stdout = os.Stdout
	command.Stderr = os.Stderr
	return command.Run()
}

func envOrDefault(key, fallback string) string {
	if value := os.Getenv(key); value != "" {
		return value
	}
	return fallback
}

func main() {
	if err := run(); err != nil {
		var exitError *exec.ExitError
		if errors.As(err, &exitError) {
			os.Exit(exitError.ExitCode())
		}
		fmt.Fprintf(os.Stderr, "goldeneye: %v\n", err)
		os.Exit(1)
	}
}
