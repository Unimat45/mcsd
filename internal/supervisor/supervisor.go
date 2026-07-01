// Package supervisor manages the lifecycle of configured Minecraft servers:
// spawning them as child processes, enforcing a state machine, restarting on
// unexpected exits with backoff and crash-loop protection, and stopping them
// gracefully.
package supervisor

import (
	"fmt"
	"log"
	"os"
	"path/filepath"
	"sort"
	"sync"

	"github.com/mathieu/mcsd/internal/config"
	"github.com/mathieu/mcsd/internal/uuid"
)

// Supervisor owns a set of server Instances keyed by id.
type Supervisor struct {
	logger     *log.Logger
	home       string // daemon data dir; per-server logs live under <home>/logs/<id>
	serversDir string // base dir; each server's files live under <serversDir>/<id>

	mu        sync.Mutex
	instances map[string]*Instance
}

// New creates an empty Supervisor. home holds daemon state (logs, servers.json);
// serversDir is the base under which per-server folders are created.
func New(logger *log.Logger, home, serversDir string) *Supervisor {
	return &Supervisor{
		logger:     logger,
		home:       home,
		serversDir: serversDir,
		instances:  make(map[string]*Instance),
	}
}

// logDir returns the persistent-log directory for a server id.
func (s *Supervisor) logDir(id string) string {
	return filepath.Join(s.home, "logs", id)
}

// Load registers instances for each config, replacing any existing set. It
// does not start them; call AutoStart or Start.
func (s *Supervisor) Load(cfgs []config.ServerConfig) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.instances = make(map[string]*Instance, len(cfgs))
	for _, c := range cfgs {
		s.instances[c.ID] = NewInstance(c, s.logger, s.logDir(c.ID))
	}
}

// Get returns the instance for id.
func (s *Supervisor) Get(id string) (*Instance, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	inst, ok := s.instances[id]
	return inst, ok
}

// List returns all instances sorted by id.
func (s *Supervisor) List() []*Instance {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := make([]*Instance, 0, len(s.instances))
	for _, inst := range s.instances {
		out = append(out, inst)
	}
	sort.Slice(out, func(a, b int) bool {
		return out[a].cfg.ID < out[b].cfg.ID
	})
	return out
}

// Add registers a new server and returns its instance. The id is generated
// (UUIDv7) when empty, and the working directory is always assigned by the
// daemon as <serversDir>/<id> — callers cannot choose it. The directory is
// created and the config persisted to servers.json.
func (s *Supervisor) Add(cfg config.ServerConfig) (*Instance, error) {
	if cfg.ID == "" {
		cfg.ID = uuid.NewV7()
	}
	cfg.Directory = filepath.Join(s.serversDir, cfg.ID)

	s.mu.Lock()
	if _, exists := s.instances[cfg.ID]; exists {
		s.mu.Unlock()
		return nil, fmt.Errorf("server %q already exists", cfg.ID)
	}
	if err := os.MkdirAll(cfg.Directory, 0o755); err != nil {
		s.mu.Unlock()
		return nil, err
	}
	inst := NewInstance(cfg, s.logger, s.logDir(cfg.ID))
	s.instances[cfg.ID] = inst
	s.mu.Unlock()

	if err := s.save(); err != nil {
		return nil, err
	}
	return inst, nil
}

// Update replaces an existing server's config and persists it. Launch-affecting
// changes take effect on the next start.
func (s *Supervisor) Update(cfg config.ServerConfig) error {
	inst, ok := s.Get(cfg.ID)
	if !ok {
		return fmt.Errorf("server %q not found", cfg.ID)
	}
	inst.UpdateConfig(cfg)
	return s.save()
}

// Remove deletes a server. It must not be running (stop it first). When purge
// is true, the server's files (working directory) and persisted logs are also
// deleted from disk — this is irreversible. The shared jar cache is untouched.
func (s *Supervisor) Remove(id string, purge bool) error {
	inst, ok := s.Get(id)
	if !ok {
		return fmt.Errorf("server %q not found", id)
	}
	switch inst.State() {
	case StateRunning, StateStarting, StateStopping:
		return fmt.Errorf("stop server %q before deleting (state=%s)", id, inst.State())
	}

	// Ensure the supervise loop is fully torn down (e.g. a crashed server may be
	// mid-restart-backoff) so it can't relaunch after we drop it.
	inst.Stop()
	inst.CloseStore()

	dir := inst.Config().Directory

	s.mu.Lock()
	delete(s.instances, id)
	s.mu.Unlock()

	if err := s.save(); err != nil {
		return err
	}

	if purge {
		if dir != "" {
			if err := os.RemoveAll(dir); err != nil {
				return fmt.Errorf("delete server files: %w", err)
			}
		}
		if err := os.RemoveAll(s.logDir(id)); err != nil {
			return fmt.Errorf("delete server logs: %w", err)
		}
	}
	return nil
}

// save writes the current set of server configs to servers.json.
func (s *Supervisor) save() error {
	cfgs := make([]config.ServerConfig, 0)
	for _, inst := range s.List() {
		cfgs = append(cfgs, inst.Config())
	}
	return config.Save(s.home, &config.File{Servers: cfgs})
}

// AutoStart starts every instance whose config has AutoStart set.
func (s *Supervisor) AutoStart() {
	for _, inst := range s.List() {
		if inst.Config().AutoStart {
			s.logger.Printf("autostarting %q", inst.Config().ID)
			inst.Start()
		}
	}
}

// StopAll gracefully stops every instance. Stops run concurrently so a slow
// shutdown doesn't hold up the others.
func (s *Supervisor) StopAll() {
	var wg sync.WaitGroup
	for _, inst := range s.List() {
		wg.Add(1)
		go func(in *Instance) {
			defer wg.Done()
			in.Stop()
		}(inst)
	}
	wg.Wait()
}

// CloseStores flushes and closes every instance's persistent log. Call after
// StopAll during daemon shutdown.
func (s *Supervisor) CloseStores() {
	for _, inst := range s.List() {
		inst.CloseStore()
	}
}
