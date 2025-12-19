# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs` wires the CLI to the daemon and shared history state.
- Library modules live in `src/lib/`: `cli.rs` (clap parser), `daemon.rs` (Hyprland event listener), `event_history.rs` (ring buffer with cursor), `hypr_events.rs`, `socket.rs`, and `types.rs`.
- Unit tests sit beside code (see `src/lib/event_history.rs`); add new integration tests under `tests/`.
- Binary target: `hyprhist`; library target: `lib` (exported via `src/lib/lib.rs`).

## Build, Test, and Development Commands
- `cargo build` — compile the crate.
- `cargo run -- daemon focus --monitor 1` — run the daemon, limiting tracking to monitor `1`; repeat `--monitor` to include more.
- `cargo test` — run unit and integration tests.
- `cargo fmt --all` — apply rustfmt to the workspace.
- `cargo clippy --all-targets -- -D warnings` — pedantic lint gate; fixes needed before merge.
- Nix users: `nix develop` for the dev shell; `nix flake check` to run fmt/clippy/tests via flake outputs.

## Coding Style & Naming Conventions
- Rust 2024 edition; prefer idiomatic ownership and `?`-based error propagation with `anyhow::Result`.
- Snake_case for files, modules, and variables; PascalCase for types and traits; UPPER_SNAKE_CASE for constants and env vars (e.g., `RUST_LOG`).
- Keep logging structured with `log`/`env_logger`; prefer `info!` for state transitions and `error!` for failures.
- Favor small modules and functions; keep CLI parsing in `cli.rs` and Hyprland interactions in `daemon.rs`/`hypr_events.rs`.

## Testing Guidelines
- Use colocated unit tests (`#[cfg(test)] mod tests`) for module internals and edge cases; name tests after behavior (`cursor_stops_at_history_start`).
- Place black-box or CLI/daemon integration tests in `tests/*.rs`; use `#[tokio::test]` for async paths.
- Add tests when modifying event history semantics or window-tracking rules; ensure new behaviors are asserted before merging.

## Commit & Pull Request Guidelines
- Commit messages should be imperative and succinct (examples from history: “Add logging”, “add basic event listener logic for window focus changes”); keep subject ≤72 chars.
- PRs should summarize intent, list commands run (fmt, clippy, tests), and note Hyprland-specific setup if required. Link related issues; include logs or brief repro steps for daemon/CLI changes.

## Environment & Configuration Tips
- Requires a Hyprland session for live events; mock or guard Hyprland calls in tests.
- Logging defaults to `info`; override with `RUST_LOG=debug hyprhist ...` when diagnosing focus tracking.
