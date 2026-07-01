// Package provider resolves downloadable Minecraft server jars across forks
// (Vanilla via Mojang's version manifest, Paper/Folia via the PaperMC v3 "fill"
// API), caches them by checksum, and installs them into server directories.
package provider

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"time"
)

const userAgent = "mcsd/0.1 (+https://github.com/mathieu/mcsd)"

var httpClient = &http.Client{Timeout: 60 * time.Second}

// Build identifies a specific build of a fork for a Minecraft version. Forks
// without a build concept (Vanilla) don't produce these.
type Build struct {
	ID      string `json:"id"`
	Channel string `json:"channel,omitempty"`
}

// Artifact is a resolved, downloadable server jar.
type Artifact struct {
	Fork      string `json:"fork"`
	MCVersion string `json:"mcVersion"`
	Build     string `json:"build,omitempty"`
	FileName  string `json:"fileName"`
	URL       string `json:"url"`
	SHA256    string `json:"sha256,omitempty"`
	SHA1      string `json:"sha1,omitempty"`
	Size      int64  `json:"size"`
}

// Provider is a source of server jars for one fork.
type Provider interface {
	ID() string
	Name() string
	HasBuilds() bool
	Versions(ctx context.Context) ([]string, error)
	Builds(ctx context.Context, mcVersion string) ([]Build, error)
	Resolve(ctx context.Context, mcVersion, build string) (Artifact, error)
}

// Info is a provider's public description.
type Info struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	HasBuilds bool   `json:"hasBuilds"`
}

// Registry holds the available providers in a stable order.
type Registry struct {
	order []string
	m     map[string]Provider
}

// NewRegistry builds the default registry: Vanilla, Paper, Folia.
func NewRegistry() *Registry {
	r := &Registry{m: make(map[string]Provider)}
	r.register(newVanilla())
	r.register(newPaper("paper", "Paper"))
	r.register(newPaper("folia", "Folia"))
	return r
}

func (r *Registry) register(p Provider) {
	r.order = append(r.order, p.ID())
	r.m[p.ID()] = p
}

// Get returns the provider for a fork id.
func (r *Registry) Get(id string) (Provider, bool) {
	p, ok := r.m[id]
	return p, ok
}

// List returns provider descriptions in registration order.
func (r *Registry) List() []Info {
	out := make([]Info, 0, len(r.order))
	for _, id := range r.order {
		p := r.m[id]
		out = append(out, Info{ID: p.ID(), Name: p.Name(), HasBuilds: p.HasBuilds()})
	}
	return out
}

// getJSON fetches url and decodes JSON into out, with the shared client, a
// descriptive User-Agent, and context cancellation.
func getJSON(ctx context.Context, url string, out any) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return err
	}
	req.Header.Set("User-Agent", userAgent)
	req.Header.Set("Accept", "application/json")
	resp, err := httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(io.LimitReader(resp.Body, 512))
		return fmt.Errorf("GET %s: %s: %s", url, resp.Status, body)
	}
	return json.NewDecoder(resp.Body).Decode(out)
}
