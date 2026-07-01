.PHONY: all web web-deps build run dev install uninstall clean

# Install locations (override e.g. `make install PREFIX=$HOME/.local`).
# DESTDIR is honored for staged/packaged installs.
PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
INSTALL ?= install

all: build

# Install frontend dependencies (run once, or after package.json changes).
web-deps:
	cd web && npm install

# Build the Svelte SPA into internal/webui/dist (embedded by the daemon).
web:
	cd web && npm run build

# Build the daemon and CLI. Depends on the built web assets being present.
build: web
	go build -o bin/mcsd ./cmd/mcsd
	go build -o bin/mcsctl ./cmd/mcsctl

# Build everything and run the daemon.
run: build
	./bin/mcsd

# Frontend dev server (proxies /api to the daemon on :8765). Run the daemon
# separately: `go run ./cmd/mcsd`.
dev:
	cd web && npm run dev

# Install the daemon and CLI into $(BINDIR) (default /usr/local/bin; needs sudo).
install: build
	$(INSTALL) -d $(DESTDIR)$(BINDIR)
	$(INSTALL) -m 0755 bin/mcsd $(DESTDIR)$(BINDIR)/mcsd
	$(INSTALL) -m 0755 bin/mcsctl $(DESTDIR)$(BINDIR)/mcsctl

# Remove the installed binaries.
uninstall:
	rm -f $(DESTDIR)$(BINDIR)/mcsd $(DESTDIR)$(BINDIR)/mcsctl

clean:
	rm -rf bin internal/webui/dist
