package provider

import (
	"context"
	"crypto/sha1"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"hash"
	"io"
	"net/http"
	"os"
	"path/filepath"
)

// Cache stores downloaded jars keyed by checksum so multiple servers reuse the
// same file and downloads are skipped when already present and verified.
type Cache struct {
	dir string
}

// NewCache creates a jar cache rooted at dir.
func NewCache(dir string) (*Cache, error) {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, err
	}
	return &Cache{dir: dir}, nil
}

func (c *Cache) checksum(a Artifact) (algo, sum string) {
	if a.SHA256 != "" {
		return "sha256", a.SHA256
	}
	return "sha1", a.SHA1
}

// path returns the on-disk location for an artifact: <dir>/<checksum>/<name>.
func (c *Cache) path(a Artifact) string {
	_, sum := c.checksum(a)
	name := a.FileName
	if name == "" {
		name = "server.jar"
	}
	if sum == "" {
		sum = "nochecksum"
	}
	return filepath.Join(c.dir, sum, name)
}

func newHasher(algo string) hash.Hash {
	if algo == "sha1" {
		return sha1.New()
	}
	return sha256.New()
}

// verify returns true if path exists and matches the artifact's checksum.
func (c *Cache) verify(path string, a Artifact) bool {
	algo, sum := c.checksum(a)
	if sum == "" {
		return false
	}
	f, err := os.Open(path)
	if err != nil {
		return false
	}
	defer f.Close()
	h := newHasher(algo)
	if _, err := io.Copy(h, f); err != nil {
		return false
	}
	return hex.EncodeToString(h.Sum(nil)) == sum
}

// Fetch ensures the artifact is present in the cache (downloading and verifying
// if needed) and returns its path.
func (c *Cache) Fetch(ctx context.Context, a Artifact) (string, error) {
	dest := c.path(a)
	if c.verify(dest, a) {
		return dest, nil // cached and intact
	}
	if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
		return "", err
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, a.URL, nil)
	if err != nil {
		return "", err
	}
	req.Header.Set("User-Agent", userAgent)
	resp, err := httpClient.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("download %s: %s", a.URL, resp.Status)
	}

	tmp, err := os.CreateTemp(filepath.Dir(dest), ".download-*")
	if err != nil {
		return "", err
	}
	tmpName := tmp.Name()
	defer os.Remove(tmpName) // no-op after successful rename

	algo, want := c.checksum(a)
	h := newHasher(algo)
	if _, err := io.Copy(io.MultiWriter(tmp, h), resp.Body); err != nil {
		tmp.Close()
		return "", err
	}
	if err := tmp.Close(); err != nil {
		return "", err
	}

	if want != "" {
		if got := hex.EncodeToString(h.Sum(nil)); got != want {
			return "", fmt.Errorf("checksum mismatch for %s: got %s, want %s", a.FileName, got, want)
		}
	}
	if err := os.Rename(tmpName, dest); err != nil {
		return "", err
	}
	return dest, nil
}

// Install copies a cached jar into a server directory under jarName.
func Install(cachePath, serverDir, jarName string) error {
	if err := os.MkdirAll(serverDir, 0o755); err != nil {
		return err
	}
	src, err := os.Open(cachePath)
	if err != nil {
		return err
	}
	defer src.Close()

	dest := filepath.Join(serverDir, jarName)
	tmp := dest + ".tmp"
	out, err := os.Create(tmp)
	if err != nil {
		return err
	}
	if _, err := io.Copy(out, src); err != nil {
		out.Close()
		os.Remove(tmp)
		return err
	}
	if err := out.Close(); err != nil {
		os.Remove(tmp)
		return err
	}
	return os.Rename(tmp, dest)
}
