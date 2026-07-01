package provider

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"strconv"
	"strings"
)

const paperBaseURL = "https://fill.papermc.io/v3/projects"

// paperProvider talks to the PaperMC v3 "fill" API, which serves Paper, Folia,
// and other PaperMC projects with an identical shape.
type paperProvider struct {
	project string
	name    string
}

func newPaper(project, name string) *paperProvider {
	return &paperProvider{project: project, name: name}
}

func (p *paperProvider) ID() string      { return p.project }
func (p *paperProvider) Name() string    { return p.name }
func (p *paperProvider) HasBuilds() bool { return true }

// Versions flattens the API's minor-grouped version map, preserving order
// (newest first) and hiding pre-releases/release-candidates.
func (p *paperProvider) Versions(ctx context.Context) ([]string, error) {
	var resp struct {
		Versions json.RawMessage `json:"versions"`
	}
	if err := getJSON(ctx, fmt.Sprintf("%s/%s", paperBaseURL, p.project), &resp); err != nil {
		return nil, err
	}

	// versions is an object { "1.21": ["1.21.8", ...], ... } whose key order is
	// meaningful, so decode with a token walk rather than into a Go map.
	dec := json.NewDecoder(bytes.NewReader(resp.Versions))
	if _, err := dec.Token(); err != nil { // opening '{'
		return nil, err
	}
	var out []string
	for dec.More() {
		if _, err := dec.Token(); err != nil { // group key
			return nil, err
		}
		var group []string
		if err := dec.Decode(&group); err != nil {
			return nil, err
		}
		for _, v := range group {
			if !strings.Contains(v, "-") { // skip -rc/-pre/-snapshot
				out = append(out, v)
			}
		}
	}
	return out, nil
}

type paperBuild struct {
	ID        int    `json:"id"`
	Channel   string `json:"channel"`
	Downloads map[string]struct {
		Name      string `json:"name"`
		URL       string `json:"url"`
		Size      int64  `json:"size"`
		Checksums struct {
			SHA256 string `json:"sha256"`
		} `json:"checksums"`
	} `json:"downloads"`
}

func (p *paperProvider) fetchBuilds(ctx context.Context, mcVersion string) ([]paperBuild, error) {
	var builds []paperBuild
	url := fmt.Sprintf("%s/%s/versions/%s/builds", paperBaseURL, p.project, mcVersion)
	if err := getJSON(ctx, url, &builds); err != nil {
		return nil, err
	}
	return builds, nil
}

func (p *paperProvider) Builds(ctx context.Context, mcVersion string) ([]Build, error) {
	builds, err := p.fetchBuilds(ctx, mcVersion)
	if err != nil {
		return nil, err
	}
	out := make([]Build, 0, len(builds))
	for _, b := range builds {
		out = append(out, Build{ID: strconv.Itoa(b.ID), Channel: b.Channel})
	}
	return out, nil
}

// Resolve returns the download for a specific build, or the latest build when
// build is empty.
func (p *paperProvider) Resolve(ctx context.Context, mcVersion, build string) (Artifact, error) {
	builds, err := p.fetchBuilds(ctx, mcVersion)
	if err != nil {
		return Artifact{}, err
	}
	if len(builds) == 0 {
		return Artifact{}, fmt.Errorf("%s has no builds for %s", p.name, mcVersion)
	}

	var chosen *paperBuild
	if build == "" {
		chosen = &builds[0] // API returns newest first
	} else {
		for i := range builds {
			if strconv.Itoa(builds[i].ID) == build {
				chosen = &builds[i]
				break
			}
		}
	}
	if chosen == nil {
		return Artifact{}, fmt.Errorf("%s build %q not found for %s", p.name, build, mcVersion)
	}

	dl, ok := chosen.Downloads["server:default"]
	if !ok {
		return Artifact{}, fmt.Errorf("%s build %d has no server download", p.name, chosen.ID)
	}
	return Artifact{
		Fork:      p.project,
		MCVersion: mcVersion,
		Build:     strconv.Itoa(chosen.ID),
		FileName:  dl.Name,
		URL:       dl.URL,
		SHA256:    dl.Checksums.SHA256,
		Size:      dl.Size,
	}, nil
}
