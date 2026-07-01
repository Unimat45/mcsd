// Package config defines the on-disk representation of managed servers and the
// daemon's data directory layout.
package config

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
)

// ServerConfig describes a single managed Minecraft server. It is
// intentionally MC-shaped (java + jar), but JavaPath may point at any
// executable and Jar may be empty, which is handy for testing the supervisor
// against a stand-in process (see examples/fakeserver.sh).
type ServerConfig struct {
	ID          string   `json:"id"`
	Name        string   `json:"name"`
	Directory   string   `json:"directory"`             // working directory for the process
	Jar         string   `json:"jar,omitempty"`         // server jar, relative to Directory or absolute
	JavaPath    string   `json:"java_path,omitempty"`   // defaults to "java" when empty
	JavaArgs    []string `json:"java_args,omitempty"`   // e.g. ["-Xmx2G", "-Xms2G"]
	ServerArgs  []string `json:"server_args,omitempty"` // e.g. ["nogui"]
	AutoStart   bool     `json:"autostart"`             // start when the daemon boots
	AutoRestart bool     `json:"autorestart"`           // restart on unexpected exit
	// Fork/MCVersion/Build record what was installed via the version manager;
	// they are informational and set by the install endpoint.
	Fork      string `json:"fork,omitempty"`
	MCVersion string `json:"mc_version,omitempty"`
	Build     string `json:"build,omitempty"`
	// StopTimeoutSeconds is how long to wait after sending "stop" before
	// escalating to signals. Defaults to 30s when zero.
	StopTimeoutSeconds int `json:"stop_timeout_seconds,omitempty"`
}

// File is the top-level shape of servers.json.
type File struct {
	Servers []ServerConfig `json:"servers"`
}

// DefaultHome returns the daemon's data directory: $MCSD_HOME if set,
// otherwise ~/.mcsd.
func DefaultHome() (string, error) {
	if h := os.Getenv("MCSD_HOME"); h != "" {
		return h, nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".mcsd"), nil
}

// ServersPath returns the path to servers.json within a home directory.
func ServersPath(home string) string { return filepath.Join(home, "servers.json") }

// DefaultServersDir returns the base directory under which each server's files
// live (one folder per server id): $MCSD_SERVERS_DIR if set, otherwise
// <home>/servers.
func DefaultServersDir(home string) string {
	if d := os.Getenv("MCSD_SERVERS_DIR"); d != "" {
		return d
	}
	return filepath.Join(home, "servers")
}

// Load reads servers.json from home. A missing file yields an empty File and
// no error, so a fresh install starts cleanly.
func Load(home string) (*File, error) {
	path := ServersPath(home)
	data, err := os.ReadFile(path)
	if os.IsNotExist(err) {
		return &File{}, nil
	}
	if err != nil {
		return nil, fmt.Errorf("read %s: %w", path, err)
	}
	var f File
	if err := json.Unmarshal(data, &f); err != nil {
		return nil, fmt.Errorf("parse %s: %w", path, err)
	}
	return &f, nil
}

// Save atomically writes servers.json to home.
func Save(home string, f *File) error {
	if err := os.MkdirAll(home, 0o755); err != nil {
		return err
	}
	data, err := json.MarshalIndent(f, "", "  ")
	if err != nil {
		return err
	}
	path := ServersPath(home)
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, data, 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}
