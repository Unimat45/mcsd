# mcsd tasks. Run `just` to list them.
#
# Dev recipes use ./.dev as data dir and port 8766, so they never touch your
# real servers or clash with an installed daemon on 8765. Dev token: "dev".

set windows-shell := ["cmd.exe", "/c"]

dev_port := "8766"

# The Vite dev server proxies /api here.
export MCSD_URL := "http://127.0.0.1:" + dev_port

[private]
default:
    @just --list --unsorted

# Install the dashboard's npm dependencies (once, or after package changes)
[working-directory('web')]
install:
    npm ci

# Build the dashboard into web/build (embedded into the binary)
[working-directory('web')]
web:
    npm run build

# Release binary with the dashboard embedded: target/release/mcsd
build: web
    cargo build --release --locked

# Debug daemon on .dev (serves web/build from disk: run `just web`, then reload)
dev: dev-config
    cargo run -- --data-dir .dev daemon

# Dashboard with hot reload on http://localhost:5173 (needs `just dev` running)
[working-directory('web')]
dev-web:
    npm run dev

# CLI against the dev daemon, e.g. `just cli ls` or `just cli logs survival -f`
cli *args: dev-config
    @cargo run -q -- --data-dir .dev {{ args }}

# Same gates as CI: svelte-check, clippy (warnings = errors), tests
check: check-web
    cargo clippy --locked --all-targets -- -D warnings
    cargo test --locked

[private]
[working-directory('web')]
check-web:
    npm run check

# Rust unit tests only
test:
    cargo test --locked

# Linux release binaries (x86_64 + arm64, glibc 2.17) via cargo-zigbuild
dist-linux: web
    cargo zigbuild --release --locked --target x86_64-unknown-linux-gnu.2.17
    cargo zigbuild --release --locked --target aarch64-unknown-linux-gnu.2.17

# Delete the dev data dir (dev servers, worlds, logs)
[confirm("Delete .dev (all dev servers and worlds)?")]
dev-reset:
    {{ if os_family() == "windows" { "rmdir /s /q .dev" } else { "rm -rf .dev" } }}

# Remove Rust build artifacts
clean:
    cargo clean

[private]
[unix]
dev-config:
    @mkdir -p .dev && [ -f .dev/config.toml ] || printf 'bind = "127.0.0.1:{{ dev_port }}"\ntoken = "dev"\n' > .dev/config.toml

[private]
[windows]
dev-config:
    @if not exist .dev mkdir .dev
    @if not exist .dev\config.toml (echo bind = "127.0.0.1:{{ dev_port }}"& echo token = "dev")> .dev\config.toml
