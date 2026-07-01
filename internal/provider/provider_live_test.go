package provider

import (
	"context"
	"os"
	"testing"
	"time"
)

// These tests hit the real Mojang and PaperMC APIs. They run only when
// MCSD_LIVE=1 so the normal test suite stays offline and fast.
func liveOrSkip(t *testing.T) {
	if os.Getenv("MCSD_LIVE") != "1" {
		t.Skip("set MCSD_LIVE=1 to run network tests")
	}
}

func TestLiveVanilla(t *testing.T) {
	liveOrSkip(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	v := newVanilla()
	versions, err := v.Versions(ctx)
	if err != nil || len(versions) == 0 {
		t.Fatalf("Versions: %v (n=%d)", err, len(versions))
	}
	art, err := v.Resolve(ctx, versions[0], "")
	if err != nil {
		t.Fatalf("Resolve %s: %v", versions[0], err)
	}
	if art.URL == "" || art.SHA1 == "" {
		t.Fatalf("incomplete artifact: %+v", art)
	}
	t.Logf("vanilla latest=%s url=%s sha1=%s size=%d", versions[0], art.URL, art.SHA1, art.Size)
}

func TestLivePaper(t *testing.T) {
	liveOrSkip(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	p := newPaper("paper", "Paper")
	versions, err := p.Versions(ctx)
	if err != nil || len(versions) == 0 {
		t.Fatalf("Versions: %v (n=%d)", err, len(versions))
	}
	builds, err := p.Builds(ctx, versions[0])
	if err != nil || len(builds) == 0 {
		t.Fatalf("Builds %s: %v (n=%d)", versions[0], err, len(builds))
	}
	art, err := p.Resolve(ctx, versions[0], "")
	if err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	if art.URL == "" || art.SHA256 == "" {
		t.Fatalf("incomplete artifact: %+v", art)
	}
	t.Logf("paper latest=%s build=%s url=%s sha256=%s", versions[0], art.Build, art.URL, art.SHA256)
}
