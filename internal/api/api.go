// Package api exposes the daemon over HTTP: a JSON REST surface for server
// lifecycle, CRUD, status, log history, and command injection, plus a
// Server-Sent Events endpoint that replays recent history then streams live
// console output (reconnect-safe, survives server restarts).
package api

import (
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"strconv"
	"time"

	"github.com/mathieu/mcsd/internal/config"
	"github.com/mathieu/mcsd/internal/provider"
	"github.com/mathieu/mcsd/internal/supervisor"
)

// Server adapts a supervisor to an http.Handler.
type Server struct {
	sup      *supervisor.Supervisor
	logger   *log.Logger
	registry *provider.Registry
	cache    *provider.Cache
}

// New creates an API server over sup, with the given version providers and jar
// cache.
func New(sup *supervisor.Supervisor, logger *log.Logger, registry *provider.Registry, cache *provider.Cache) *Server {
	return &Server{sup: sup, logger: logger, registry: registry, cache: cache}
}

// Handler builds the HTTP routes (Go 1.22+ method+pattern routing).
func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/health", s.health)
	mux.HandleFunc("GET /api/servers", s.listServers)
	mux.HandleFunc("POST /api/servers", s.createServer)
	mux.HandleFunc("GET /api/servers/{id}", s.getServer)
	mux.HandleFunc("PUT /api/servers/{id}", s.updateServer)
	mux.HandleFunc("DELETE /api/servers/{id}", s.deleteServer)
	mux.HandleFunc("POST /api/servers/{id}/start", s.startServer)
	mux.HandleFunc("POST /api/servers/{id}/stop", s.stopServer)
	mux.HandleFunc("POST /api/servers/{id}/restart", s.restartServer)
	mux.HandleFunc("POST /api/servers/{id}/command", s.sendCommand)
	mux.HandleFunc("GET /api/servers/{id}/logs", s.getLogs)
	mux.HandleFunc("GET /api/servers/{id}/stream", s.streamLogs)
	mux.HandleFunc("GET /api/servers/{id}/properties", s.getProperties)
	mux.HandleFunc("PUT /api/servers/{id}/properties", s.putProperties)
	mux.HandleFunc("GET /api/servers/{id}/eula", s.getEula)
	mux.HandleFunc("POST /api/servers/{id}/eula", s.putEula)
	mux.HandleFunc("GET /api/providers", s.listProviders)
	mux.HandleFunc("GET /api/providers/{fork}/versions", s.providerVersions)
	mux.HandleFunc("GET /api/providers/{fork}/versions/{version}/builds", s.providerBuilds)
	mux.HandleFunc("POST /api/servers/{id}/install", s.installVersion)
	mux.HandleFunc("GET /api/java", s.javaInfo)
	return logRequests(s.logger, mux)
}

// --- DTOs ---

// StatusDTO is a server config plus its live runtime state.
type StatusDTO struct {
	config.ServerConfig
	State string `json:"state"`
}

// LineDTO is one console line, used for both history and live streaming.
type LineDTO struct {
	Ts     time.Time `json:"ts"`
	Stream string    `json:"stream"`
	Text   string    `json:"text"`
}

func statusOf(inst *supervisor.Instance) StatusDTO {
	return StatusDTO{ServerConfig: inst.Config(), State: string(inst.State())}
}

// --- handlers ---

func (s *Server) health(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
}

func (s *Server) listServers(w http.ResponseWriter, r *http.Request) {
	insts := s.sup.List()
	out := make([]StatusDTO, 0, len(insts))
	for _, inst := range insts {
		out = append(out, statusOf(inst))
	}
	writeJSON(w, http.StatusOK, out)
}

func (s *Server) getServer(w http.ResponseWriter, r *http.Request) {
	inst, ok := s.sup.Get(r.PathValue("id"))
	if !ok {
		writeError(w, http.StatusNotFound, "server not found")
		return
	}
	writeJSON(w, http.StatusOK, statusOf(inst))
}

func (s *Server) createServer(w http.ResponseWriter, r *http.Request) {
	var cfg config.ServerConfig
	if err := json.NewDecoder(r.Body).Decode(&cfg); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON: "+err.Error())
		return
	}
	// ids are auto-generated and directories are daemon-managed; ignore any
	// client-supplied values.
	cfg.ID = ""
	cfg.Directory = ""
	inst, err := s.sup.Add(cfg)
	if err != nil {
		writeError(w, http.StatusConflict, err.Error())
		return
	}
	writeJSON(w, http.StatusCreated, statusOf(inst))
}

func (s *Server) updateServer(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("id")
	if _, ok := s.sup.Get(id); !ok {
		writeError(w, http.StatusNotFound, "server not found")
		return
	}
	var cfg config.ServerConfig
	if err := json.NewDecoder(r.Body).Decode(&cfg); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON: "+err.Error())
		return
	}
	cfg.ID = id // path is authoritative
	if err := s.sup.Update(cfg); err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	inst, _ := s.sup.Get(id)
	writeJSON(w, http.StatusOK, statusOf(inst))
}

func (s *Server) deleteServer(w http.ResponseWriter, r *http.Request) {
	purge := r.URL.Query().Get("purge") == "true"
	if err := s.sup.Remove(r.PathValue("id"), purge); err != nil {
		writeError(w, http.StatusConflict, err.Error())
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

func (s *Server) startServer(w http.ResponseWriter, r *http.Request) {
	s.lifecycle(w, r, func(inst *supervisor.Instance) { inst.Start() })
}

func (s *Server) stopServer(w http.ResponseWriter, r *http.Request) {
	s.lifecycle(w, r, func(inst *supervisor.Instance) { inst.Stop() })
}

func (s *Server) restartServer(w http.ResponseWriter, r *http.Request) {
	s.lifecycle(w, r, func(inst *supervisor.Instance) { inst.Restart() })
}

func (s *Server) lifecycle(w http.ResponseWriter, r *http.Request, action func(*supervisor.Instance)) {
	inst, ok := s.sup.Get(r.PathValue("id"))
	if !ok {
		writeError(w, http.StatusNotFound, "server not found")
		return
	}
	action(inst)
	writeJSON(w, http.StatusOK, statusOf(inst))
}

func (s *Server) sendCommand(w http.ResponseWriter, r *http.Request) {
	inst, ok := s.sup.Get(r.PathValue("id"))
	if !ok {
		writeError(w, http.StatusNotFound, "server not found")
		return
	}
	var body struct {
		Command string `json:"command"`
	}
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		writeError(w, http.StatusBadRequest, "invalid JSON: "+err.Error())
		return
	}
	if body.Command == "" {
		writeError(w, http.StatusBadRequest, "command is required")
		return
	}
	if err := inst.Send(body.Command); err != nil {
		writeError(w, http.StatusConflict, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, map[string]string{"sent": body.Command})
}

func (s *Server) getLogs(w http.ResponseWriter, r *http.Request) {
	inst, ok := s.sup.Get(r.PathValue("id"))
	if !ok {
		writeError(w, http.StatusNotFound, "server not found")
		return
	}
	entries, err := inst.Tail(tailParam(r, 200))
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	out := make([]LineDTO, len(entries))
	for i, e := range entries {
		out[i] = LineDTO{Ts: e.Ts, Stream: e.Stream, Text: e.Text}
	}
	writeJSON(w, http.StatusOK, out)
}

// streamLogs replays recent history then streams live lines as SSE.
func (s *Server) streamLogs(w http.ResponseWriter, r *http.Request) {
	inst, ok := s.sup.Get(r.PathValue("id"))
	if !ok {
		writeError(w, http.StatusNotFound, "server not found")
		return
	}
	flusher, ok := w.(http.Flusher)
	if !ok {
		writeError(w, http.StatusInternalServerError, "streaming unsupported")
		return
	}

	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")

	// Subscribe before reading history so no line is lost in the gap; a line
	// may appear both in history and live at the boundary (acceptable for logs).
	subID, ch := inst.Subscribe()
	defer inst.Unsubscribe(subID)

	if history, err := inst.Tail(tailParam(r, 200)); err == nil {
		for _, e := range history {
			writeSSE(w, LineDTO{Ts: e.Ts, Stream: e.Stream, Text: e.Text})
		}
		flusher.Flush()
	}

	ping := time.NewTicker(25 * time.Second)
	defer ping.Stop()
	ctx := r.Context()
	for {
		select {
		case line, ok := <-ch:
			if !ok {
				return
			}
			writeSSE(w, LineDTO{Ts: line.Ts, Stream: line.Stream, Text: line.Text})
			flusher.Flush()
		case <-ping.C:
			fmt.Fprint(w, ": ping\n\n") // comment frame keeps the connection warm
			flusher.Flush()
		case <-ctx.Done():
			return
		}
	}
}

// --- helpers ---

func tailParam(r *http.Request, def int) int {
	if v := r.URL.Query().Get("tail"); v != "" {
		if n, err := strconv.Atoi(v); err == nil && n >= 0 {
			return n
		}
	}
	return def
}

func writeJSON(w http.ResponseWriter, code int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	_ = json.NewEncoder(w).Encode(v)
}

func writeError(w http.ResponseWriter, code int, msg string) {
	writeJSON(w, code, map[string]string{"error": msg})
}

func writeSSE(w http.ResponseWriter, line LineDTO) {
	b, err := json.Marshal(line)
	if err != nil {
		return
	}
	fmt.Fprintf(w, "data: %s\n\n", b)
}

func logRequests(logger *log.Logger, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		next.ServeHTTP(w, r)
		logger.Printf("%s %s", r.Method, r.URL.Path)
	})
}
