// Command mcsd is the Minecraft server supervisor daemon.
//
// It loads server definitions from $MCSD_HOME/servers.json (default
// ~/.mcsd/servers.json), autostarts the ones marked for it, serves an HTTP +
// SSE API (default 127.0.0.1:8765, override with $MCSD_ADDR), and shuts
// everything down gracefully on SIGINT/SIGTERM.
package main

import (
	"context"
	"errors"
	"log"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"path/filepath"

	"github.com/mathieu/mcsd/internal/api"
	"github.com/mathieu/mcsd/internal/config"
	"github.com/mathieu/mcsd/internal/provider"
	"github.com/mathieu/mcsd/internal/supervisor"
	"github.com/mathieu/mcsd/internal/webui"
)

const defaultAddr = "127.0.0.1:8765"

func main() {
	logger := log.New(os.Stdout, "", log.LstdFlags)

	home, err := config.DefaultHome()
	if err != nil {
		logger.Fatalf("resolve home dir: %v", err)
	}
	if err := os.MkdirAll(home, 0o755); err != nil {
		logger.Fatalf("create home dir %s: %v", home, err)
	}
	logger.Printf("mcsd starting, home=%s", home)

	f, err := config.Load(home)
	if err != nil {
		logger.Fatalf("load config: %v", err)
	}
	if len(f.Servers) == 0 {
		logger.Printf("no servers defined in %s — add some via the API or file", config.ServersPath(home))
	}

	serversDir := config.DefaultServersDir(home)
	if err := os.MkdirAll(serversDir, 0o755); err != nil {
		logger.Fatalf("create servers dir %s: %v", serversDir, err)
	}
	logger.Printf("servers directory: %s", serversDir)

	sup := supervisor.New(logger, home, serversDir)
	sup.Load(f.Servers)
	sup.AutoStart()

	addr := os.Getenv("MCSD_ADDR")
	if addr == "" {
		addr = defaultAddr
	}
	registry := provider.NewRegistry()
	cache, err := provider.NewCache(filepath.Join(home, "cache", "jars"))
	if err != nil {
		logger.Fatalf("jar cache: %v", err)
	}

	webHandler, err := webui.Handler()
	if err != nil {
		logger.Fatalf("web UI: %v", err)
	}
	root := http.NewServeMux()
	root.Handle("/api/", api.New(sup, logger, registry, cache).Handler())
	root.Handle("/", webHandler)

	httpSrv := &http.Server{Addr: addr, Handler: root}
	go func() {
		logger.Printf("mcsd listening on http://%s", addr)
		if err := httpSrv.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
			logger.Fatalf("http server: %v", err)
		}
	}()

	// Block until a termination signal, then shut down gracefully.
	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)
	sig := <-sigCh
	logger.Printf("received %s; shutting down", sig)

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_ = httpSrv.Shutdown(ctx)

	sup.StopAll()
	sup.CloseStores()
	logger.Printf("mcsd stopped")
}
