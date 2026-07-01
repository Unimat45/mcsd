package provider

import (
	"context"
	"fmt"
	"sync"
	"time"
)

const (
	vanillaManifestURL = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json"
	manifestTTL        = 10 * time.Minute
)

type vanillaManifest struct {
	Latest struct {
		Release  string `json:"release"`
		Snapshot string `json:"snapshot"`
	} `json:"latest"`
	Versions []struct {
		ID   string `json:"id"`
		Type string `json:"type"`
		URL  string `json:"url"`
	} `json:"versions"`
}

type vanillaVersionDetail struct {
	Downloads struct {
		Server struct {
			SHA1 string `json:"sha1"`
			Size int64  `json:"size"`
			URL  string `json:"url"`
		} `json:"server"`
	} `json:"downloads"`
}

type vanillaProvider struct {
	mu        sync.Mutex
	manifest  *vanillaManifest
	fetchedAt time.Time
}

func newVanilla() *vanillaProvider { return &vanillaProvider{} }

func (v *vanillaProvider) ID() string      { return "vanilla" }
func (v *vanillaProvider) Name() string    { return "Vanilla" }
func (v *vanillaProvider) HasBuilds() bool { return false }

func (v *vanillaProvider) getManifest(ctx context.Context) (*vanillaManifest, error) {
	v.mu.Lock()
	defer v.mu.Unlock()
	if v.manifest != nil && time.Since(v.fetchedAt) < manifestTTL {
		return v.manifest, nil
	}
	var m vanillaManifest
	if err := getJSON(ctx, vanillaManifestURL, &m); err != nil {
		return nil, err
	}
	v.manifest = &m
	v.fetchedAt = time.Now()
	return &m, nil
}

// Versions returns release versions only (snapshots are hidden by default),
// newest first as Mojang orders them.
func (v *vanillaProvider) Versions(ctx context.Context) ([]string, error) {
	m, err := v.getManifest(ctx)
	if err != nil {
		return nil, err
	}
	var out []string
	for _, ver := range m.Versions {
		if ver.Type == "release" {
			out = append(out, ver.ID)
		}
	}
	return out, nil
}

// Builds: Vanilla has no build concept.
func (v *vanillaProvider) Builds(ctx context.Context, mcVersion string) ([]Build, error) {
	return nil, nil
}

func (v *vanillaProvider) Resolve(ctx context.Context, mcVersion, _ string) (Artifact, error) {
	m, err := v.getManifest(ctx)
	if err != nil {
		return Artifact{}, err
	}
	var url string
	for _, ver := range m.Versions {
		if ver.ID == mcVersion {
			url = ver.URL
			break
		}
	}
	if url == "" {
		return Artifact{}, fmt.Errorf("vanilla version %q not found", mcVersion)
	}
	var detail vanillaVersionDetail
	if err := getJSON(ctx, url, &detail); err != nil {
		return Artifact{}, err
	}
	srv := detail.Downloads.Server
	if srv.URL == "" {
		return Artifact{}, fmt.Errorf("vanilla version %q has no server download", mcVersion)
	}
	return Artifact{
		Fork:      "vanilla",
		MCVersion: mcVersion,
		FileName:  "server.jar",
		URL:       srv.URL,
		SHA1:      srv.SHA1,
		Size:      srv.Size,
	}, nil
}
