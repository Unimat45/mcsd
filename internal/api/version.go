package api

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"time"

	"github.com/mathieu/mcsd/internal/config"
	"github.com/mathieu/mcsd/internal/provider"
	"github.com/mathieu/mcsd/internal/supervisor"
)

// listProviders returns the available forks.
func (s *Server) listProviders(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, s.registry.List())
}

func (s *Server) providerVersions(w http.ResponseWriter, r *http.Request) {
	p, ok := s.registry.Get(r.PathValue("fork"))
	if !ok {
		writeError(w, http.StatusNotFound, "unknown fork")
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), 30*time.Second)
	defer cancel()
	versions, err := p.Versions(ctx)
	if err != nil {
		writeError(w, http.StatusBadGateway, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"versions": versions})
}

func (s *Server) providerBuilds(w http.ResponseWriter, r *http.Request) {
	p, ok := s.registry.Get(r.PathValue("fork"))
	if !ok {
		writeError(w, http.StatusNotFound, "unknown fork")
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), 30*time.Second)
	defer cancel()
	builds, err := p.Builds(ctx, r.PathValue("version"))
	if err != nil {
		writeError(w, http.StatusBadGateway, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"builds": builds})
}

type installRequest struct {
	Fork      string `json:"fork"`
	MCVersion string `json:"mcVersion"`
	Build     string `json:"build"`
	DryRun    bool   `json:"dryRun"`
}

type installResponse struct {
	Artifact provider.Artifact `json:"artifact"`
	Warnings []string          `json:"warnings"`
	Server   *StatusDTO        `json:"server,omitempty"`
}

// installVersion resolves and (unless dryRun) downloads + installs a server jar,
// updating the server's config. The server must be stopped.
func (s *Server) installVersion(w http.ResponseWriter, r *http.Request) {
	inst, ok := s.sup.Get(r.PathValue("id"))
	if !ok {
		writeError(w, http.StatusNotFound, "server not found")
		return
	}
	var req installRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON: "+err.Error())
		return
	}
	p, ok := s.registry.Get(req.Fork)
	if !ok {
		writeError(w, http.StatusBadRequest, "unknown fork")
		return
	}
	if inst.State() != supervisor.StateStopped {
		writeError(w, http.StatusConflict, "server must be stopped before changing version")
		return
	}

	ctx, cancel := context.WithTimeout(r.Context(), 5*time.Minute)
	defer cancel()

	artifact, err := p.Resolve(ctx, req.MCVersion, req.Build)
	if err != nil {
		writeError(w, http.StatusBadGateway, err.Error())
		return
	}

	cfg := inst.Config()
	warnings := s.installWarnings(cfg, req.MCVersion)

	if req.DryRun {
		writeJSON(w, http.StatusOK, installResponse{Artifact: artifact, Warnings: warnings})
		return
	}

	cachePath, err := s.cache.Fetch(ctx, artifact)
	if err != nil {
		writeError(w, http.StatusBadGateway, err.Error())
		return
	}
	jarName := "server.jar"
	if err := provider.Install(cachePath, cfg.Directory, jarName); err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}

	cfg.Jar = jarName
	cfg.Fork = artifact.Fork
	cfg.MCVersion = artifact.MCVersion
	cfg.Build = artifact.Build
	if err := s.sup.Update(cfg); err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}

	status := statusOf(inst)
	writeJSON(w, http.StatusOK, installResponse{Artifact: artifact, Warnings: warnings, Server: &status})
}

// installWarnings flags version downgrades and Java incompatibility.
func (s *Server) installWarnings(cfg config.ServerConfig, targetMC string) []string {
	var warnings []string
	if cfg.MCVersion != "" && provider.IsDowngrade(targetMC, cfg.MCVersion) {
		warnings = append(warnings, fmt.Sprintf(
			"Downgrading from %s to %s can corrupt existing world data — back up the world first.",
			cfg.MCVersion, targetMC))
	}
	rec := provider.RecommendedJava(targetMC)
	javaPath := cfg.JavaPath
	if javaPath == "" {
		javaPath = "java"
	}
	if major, _, err := provider.JavaMajor(javaPath); err == nil && major > 0 && rec > 0 && major < rec {
		warnings = append(warnings, fmt.Sprintf(
			"Minecraft %s recommends Java %d, but %q is Java %d. The server may fail to start.",
			targetMC, rec, javaPath, major))
	}
	return warnings
}

func (s *Server) javaInfo(w http.ResponseWriter, r *http.Request) {
	resp := map[string]any{"installed": provider.DetectJavas()}
	if mc := r.URL.Query().Get("mc"); mc != "" {
		resp["recommended"] = provider.RecommendedJava(mc)
	}
	writeJSON(w, http.StatusOK, resp)
}
