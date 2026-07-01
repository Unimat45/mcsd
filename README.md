# mcsd — Minecraft server supervisor daemon

A pm2-like process manager for Minecraft servers: a long-lived daemon that
keeps servers alive, captures their console output, and (in later phases)
exposes a web UI for logs, version/fork selection, and `server.properties`
editing.

## Status

**Phase 1 — supervisor core (done).** The daemon can spawn a server as a child
process, enforce a lifecycle state machine, capture stdout/stderr, restart on
unexpected exits with exponential backoff + a crash-loop breaker, and stop
gracefully (`stop` → SIGTERM → SIGKILL).

**Phase 2 — log persistence (done).** Console output is persisted per server to
JSON Lines under `<home>/logs/<id>/console.jsonl`, rotated by size and gzipped,
so history survives daemon/web-app restarts. `Instance.Tail(n)` reads the most
recent N lines across the current file and rotated archives.

**Phase 3 — HTTP API + CLI (done).** The daemon serves a stdlib-only HTTP API
(default `127.0.0.1:8765`, override with `$MCSD_ADDR`): REST for lifecycle,
status, CRUD, and command injection, plus a Server-Sent Events stream that
replays recent history then streams live console lines (reconnect-safe, and it
survives server restarts via a per-instance broker). The `mcsctl` client wraps
it (`list`, `start`, `stop`, `restart`, `send`, `logs [--follow]`, `rm`).

**Phase 4 — web UI (done).** A Svelte 5 SPA (built with Vite) served by the
daemon at `/`, embedded in the binary via `go:embed`. Sidebar of servers with
live state, start/stop/restart controls, and a streaming console (SSE) with a
command input. Source in `web/`, built into `internal/webui/dist`.

**Phase 5 — server.properties editing (done).** A comment/order-preserving
round-trip parser (`internal/properties`) plus a curated typed schema (bools,
ints, enums like `difficulty`/`gamemode`, strings) drive a Properties tab in
the UI with proper widgets. Unknown keys are preserved and editable as text.
EULA acceptance is surfaced and writable. API: `GET|PUT
/api/servers/{id}/properties`, `GET|POST /api/servers/{id}/eula`.

**Phase 6 — version/fork providers (done).** A provider interface
(`internal/provider`) resolves server jars from Mojang (Vanilla) and the
PaperMC v3 API (Paper, Folia), caches them by checksum with verification,
installs into the server dir, and records fork/version/build in the config.
Detects installed JDKs, recommends a Java version per Minecraft release, and
warns on downgrades and Java mismatches (dry-run preview). A Version tab drives
it in the UI. API: `GET /api/providers`, `.../versions`, `.../builds`, `POST
/api/servers/{id}/install`, `GET /api/java`.

Roadmap: Phase 7 backups/auth.

## API

```
GET    /api/health
GET    /api/servers                    list + state
POST   /api/servers                    create (JSON ServerConfig)
GET    /api/servers/{id}               detail + state
PUT    /api/servers/{id}               update config
DELETE /api/servers/{id}               remove (must be stopped)
POST   /api/servers/{id}/start|stop|restart
POST   /api/servers/{id}/command       body: {"command":"say hi"}
GET    /api/servers/{id}/logs?tail=N   recent history (JSON)
GET    /api/servers/{id}/stream?tail=N SSE: history then live lines
GET    /api/servers/{id}/properties    values + order + typed schema
PUT    /api/servers/{id}/properties    body: {"values":{...}} (round-trip safe)
GET    /api/servers/{id}/eula          {accepted}
POST   /api/servers/{id}/eula          body: {"accepted":true}
GET    /api/providers                  available forks
GET    /api/providers/{fork}/versions
GET    /api/providers/{fork}/versions/{version}/builds
POST   /api/servers/{id}/install       body: {fork,mcVersion,build,dryRun}
GET    /api/java?mc=<version>          detected JDKs + recommended major
```

## mcsctl

```sh
mcsctl list
mcsctl start <id>
mcsctl logs <id> --follow
mcsctl send <id> say hello
```

## Layout

```
cmd/mcsd/            daemon binary
internal/config/     servers.json model + data-dir resolution
internal/console/    stdin/stdout capture, ring buffer, subscribers
internal/supervisor/ process lifecycle, state machine, restart policy
examples/            fakeserver.sh + sample servers.json for testing
```

## Configuration

The daemon reads `$MCSD_HOME/servers.json` (default `~/.mcsd/servers.json`):

```json
{
  "servers": [
    {
      "id": "survival",
      "name": "Survival",
      "directory": "/srv/mc/survival",
      "jar": "paper.jar",
      "java_path": "java",
      "java_args": ["-Xmx4G", "-Xms4G"],
      "server_args": ["nogui"],
      "autostart": true,
      "autorestart": true,
      "stop_timeout_seconds": 30
    }
  ]
}
```

## Build & run

The web UI must be built before the daemon (its assets are embedded via
`go:embed`). The Makefile handles ordering:

```sh
make web-deps   # once: install frontend deps
make build      # builds the SPA, then the Go binaries into bin/
make run        # build + run the daemon (serves UI + API on :8765)
```

Then open http://127.0.0.1:8765.

### Frontend development

```sh
go run ./cmd/mcsd   # terminal 1: the daemon on :8765
make dev            # terminal 2: Vite dev server, proxies /api to the daemon
```

## Try it without Minecraft

`examples/fakeserver.sh` stands in for a real server (accepts `stop`, and
`crash` to force a non-zero exit). Point `java_path` at it with an empty `jar`,
as in `examples/servers.json`, and the supervisor will drive it exactly like a
JVM.
