# mcsd

A pm2-style daemon for Minecraft servers: create, run and supervise Paper,
Vanilla, Fabric or custom servers from a CLI or a web dashboard. It ships as
a single binary for Windows, Linux and macOS, with the dashboard embedded.

```
crates/mcsd/   Rust daemon + CLI (tokio, axum)
web/           SvelteKit dashboard (static SPA, embedded into the binary)
```

## Features

- **One-command servers**: downloads the jar (Mojang / PaperMC Fill v3 /
  Fabric meta, checksums verified), writes `eula.txt` and `server.properties`.
- **Java handled for you**: works out which Java version each Minecraft
  version needs (Mojang/Paper metadata), uses an installed JDK if it matches,
  and otherwise downloads Eclipse Temurin into the data dir.
- **Supervision**: live state tracking (starting → running on `Done (…)!`),
  player list, CPU/RAM per server, crash restart with backoff (2s → 60s,
  gives up after 5 crashes in 10 minutes), `/stop` in-game is respected.
- **Graceful everywhere**: stopping sends `stop` on stdin (worlds get saved)
  and only kills after a timeout. Stopping the daemon stops all servers
  gracefully and restarts them on the next boot.
- **No orphans**: on Windows, a kill-on-close Job Object ties the servers to
  the daemon. On Linux, `PR_SET_PDEATHSIG` does it. Elsewhere, leftover PIDs
  are cleaned up on the next boot.
- **Logs**: console output goes to rotating files
  (`logs/servers/<name>/console.log`, 10 MB × 5) and to an in-memory ring
  buffer of 2000 lines. The dashboard backfills from the buffer, then streams
  over WebSocket.
- **Service install**: systemd (user or system), launchd, or Windows service.

## Quick start

```sh
cd web && npm install && npm run build && cd ..
cargo build --release
./target/release/mcsd daemon                 # foreground; Ctrl+C stops servers gracefully
./target/release/mcsd dashboard              # prints the login token for http://127.0.0.1:8765
```

CLI (in another terminal):

```sh
mcsd create survival --kind paper --memory 4096 --accept-eula --start --autostart
mcsd ls
mcsd logs survival -f
mcsd send survival say hello
mcsd stop survival
mcsd set survival --memory 6144 --jvm-arg=-XX:+UseG1GC
mcsd versions fabric
mcsd java ls
mcsd service install            # start at boot (Windows: elevated prompt)
```

To run a Forge/NeoForge/Purpur server, use `--kind custom --jar path/to/server.jar --version 1.21.1`.

## Data directory

Defaults to the OS app-data dir. Override it with `--data-dir` or `MCSD_HOME`.

| OS      | Default                                   |
|---------|-------------------------------------------|
| Windows | `%APPDATA%\mcsd\data`                     |
| Linux   | `~/.local/share/mcsd`                     |
| macOS   | `~/Library/Application Support/mcsd`      |

Inside it:

- `config.toml`: listen address and API token
- `servers.json`: the registry
- `servers/<name>/`: server files
- `logs/`
- `java/`: runtimes downloaded by mcsd

## Security

The API listens on `127.0.0.1:8765` by default, and every call needs the
bearer token from `config.toml`. Anyone with the token has console access,
which on most servers means operator rights. If you change `bind` to expose
it, put a TLS reverse proxy in front of it.

To manage a remote host without exposing the API, tunnel it over SSH and
paste the token printed by `mcsd dashboard` (run on that host):

```sh
ssh -L 8765:127.0.0.1:8765 user@minecraft-host   # then open http://127.0.0.1:8765
```

## Development

Tasks are in the [justfile](justfile) (`cargo install just`, then `just` lists them):

```sh
just install      # npm ci for the dashboard (once)
just dev          # debug daemon on ./.dev, port 8766, token "dev" (your real servers are untouched)
just dev-web      # dashboard with hot reload on http://localhost:5173 (proxies to the dev daemon)
just cli ls       # CLI against the dev daemon
just check        # svelte-check + clippy (warnings = errors) + tests, same as CI
just build        # release binary with the dashboard embedded
```

A debug daemon serves `web/build` from disk: after `just web`, reload the page.

## Releases

`.github/workflows/build.yml` runs when `main` changes (i.e. when `develop` is
merged into it), on `v*` tags, and manually from the Actions tab. Pull requests
and other branches don't trigger it; run `just check` locally instead. It
checks the dashboard, lints and tests on Windows, Linux and macOS, then builds:

| Target | Runner |
|---|---|
| `x86_64-pc-windows-msvc` | windows |
| `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` | ubuntu + cargo-zigbuild (glibc 2.17, runs on old distros) |
| `aarch64-apple-darwin`, `x86_64-apple-darwin` | macos |

Pushing a `v*` tag (`git tag v0.1.0 && git push --tags`) publishes a GitHub
release with the archives and `SHA256SUMS`. Tags with a `-` (`v0.2.0-rc1`) are
marked as pre-releases.

## Cross-compiling locally

`ring` (TLS) needs a C toolchain for the target. The easiest option is
[cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild):

```sh
pip install ziglang && cargo install --locked cargo-zigbuild
rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
just dist-linux   # x86_64 + arm64 (Raspberry Pi / ARM VPS), glibc 2.17
```

macOS binaries need Apple's SDK to link: build them on a Mac or let CI do it.

## API

All routes are under `/api`, with `Authorization: Bearer <token>`. WebSockets
take `?token=` instead.

| Method | Path | |
|---|---|---|
| GET | `/health` | public |
| GET/POST | `/servers` | list / create (`{name, kind, version, memory_mb, port, accept_eula, autostart, start}`) |
| GET/PATCH/DELETE | `/servers/{name}` | info / update / remove (`?delete_files=true`) |
| POST | `/servers/{name}/start\|stop\|restart` | `?wait=false` returns immediately |
| POST | `/servers/{name}/kill\|reinstall` | |
| POST | `/servers/{name}/command` | `{command}` |
| GET | `/servers/{name}/logs` | `?tail=200` or `?after=<seq>` |
| GET/PATCH/PUT | `/servers/{name}/properties` | read server.properties / `{set: {k: v}, remove: [k]}` (keeps comments & order) / `{raw}` replaces the file. `server-port` stays in sync with mcsd's port |
| GET (WS) | `/servers/{name}/console` | backlog + live lines; text frames are commands |
| GET | `/versions/{kind}` | available versions |
| GET/POST | `/java`, `/java/install` | runtimes / `{major}` |
| GET | `/system` | host info |

## License

GPL-3.0-only. See [LICENSE](LICENSE).
