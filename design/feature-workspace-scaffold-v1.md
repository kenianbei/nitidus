# feature - workspace scaffold - v1

Establish the cargo workspace for nitidus: crate boundaries, pinned
dependencies, lint policy, and a minimal compiling skeleton with green tests, so
every subsequent roadmap item (documentation/roadmap.md, Phase 1 item 1) lands
into a structured, buildable workspace. This is roadmap item 1a.1 — the first
code in the repository.

## 1. Current Design

The repository contains no Rust code. Present today:

- `documentation/` — specification.md (core features, dependencies), roadmap.md
  (build order), plus analysis docs (rust-libraries.md, ratatui-frameworks.md,
  persistence.md, neomutt/aerc/gmail/outlook).
- `.claude/rules/` — engineering standards (code.md, rust.md, testing.md,
  comments.md, git.md, contributing.md) that the workspace tooling must enforce
  (clippy clean, no unwrap in production code, fns ≤30 lines, files ≤300 lines).
- No `Cargo.toml`, no `src/`, no CI.

The sibling project `../vcard_tui` proves the UI stack works and pins the
known-good versions (bevy 0.18 with `default-features = false`, features
`bevy_log` + `bevy_state`; bevy_ratatui 0.11; plurimus 0.1 feature `ui`;
ratatui 0.30 features `crossterm`, `palette`, `unstable-widget-ref`;
ratatui-image 11; tui-prompts 0.6; image 0.25; edition 2024). Per R1, it is
a POC slated for abandonment: it serves as a version reference only — no
code or files are imported from it, in this scaffold or any later item.

## 2. Proposal

A four-crate cargo workspace:

```
nitidus/
├── Cargo.toml               # [workspace] members = ["crates/*"], resolver 3
└── crates/
    ├── nitidus/             # binary `nitidus`: entry point, logging init, app wiring
    ├── nitidus-ui-kit/      # theme/layout/builders/widgets (built in item 1a.2)
    ├── nitidus-mail/        # mail engine + MailBackend trait; ZERO bevy deps
    └── nitidus-contacts/    # contact domain + UI plugins (built in item 1e.21)
```

Key decisions encoded here:

- **`[workspace.package]`** shares edition 2024, rust-version, license, authors,
  repository across crates.
- **`[workspace.dependencies]`** is the single version authority; member crates
  reference `workspace = true` only. Scaffold pins the foundation set only (UI
  stack at vcard_tui's proven versions, plus
  tokio/flume/tokio-util/serde/toml/thiserror/anyhow/tracing/
  tracing-subscriber/etcetera). Protocol, storage, and content crates (io-\*,
  rusqlite, mail-parser, html2text, …) enter when their roadmap item starts, not
  before.
- **`nitidus-mail` must not depend on bevy** — enforced by a smoke test
  asserting the crate graph (via `cargo tree` in CI later; documented invariant
  now).
- **Lint policy in the workspace root** (`[workspace.lints]`,
  `lints.workspace = true` in every member): clippy `unwrap_used` and
  `expect_used` **hard-deny** (rules/rust.md forbids them in production
  code; test modules opt out with a module-level
  `#![allow(clippy::unwrap_used, clippy::expect_used)]`),
  `cargo clippy --workspace` must pass with no warnings.
- **Formatting enforced**: a committed `rustfmt.toml`
  (`style_edition = "2024"`, defaults otherwise); `cargo fmt --check` is
  part of verification.
- **Dual license**: `MIT OR Apache-2.0` in `[workspace.package]`, with
  `LICENSE-MIT` and `LICENSE-APACHE` texts at the repository root
  (replacing the fork's implied MIT-only, which shipped no license file).
- **Binary crate stays thin** (rules/testing.md §13): `main.rs` calls into
  `nitidus` lib code; scaffold `main` initializes tracing to a log file in the
  XDG state directory (etcetera) and reports version — the bevy app shell is
  item 1a.2, not this doc.
- Each library crate ships a placeholder module plus one smoke test so
  `CARGO_INCREMENTAL=0 cargo test --workspace` runs green with nonzero pass
  counts from day one.

Out of scope: CI pipelines, crates.io publishing metadata beyond the shared
package fields, the ui-kit/contacts ports, the bevy app shell, any io-\*
dependency.

## 3. Discussion

### 3.1 R1 Questions

1. **Binary name**: `nitidus` as the installed binary, or a short alias in the
   spirit of vcard_tui's `vct` (e.g. `pkr`)? (`pr` collides with the POSIX `pr`
   utility.)
2. **Crate naming**: `nitidus-ui-kit` / `nitidus-mail` / `nitidus-contacts` —
   confirm, or prefer shorter (`pkr-*`)? If `nitidus-ui-kit` is meant to be
   published and later adopted by vcard_tui, is `nitidus-ui-kit` the name you
   want on crates.io, or something neutral (e.g. `plurimus-kit`)?
3. **License**: what license for the workspace (vcard_tui's choice? dual MIT OR
   Apache-2.0 is the Rust-ecosystem default)? Affects `[workspace.package]` now
   and ui-kit publishing later.
4. **Version pinning strategy**: caret ranges at known-good minimums (vcard_tui
   style, `bevy = "0.18"`) or exact pins (`=0.18.x`) given bevy-orbit churn?
   Proposal assumes caret + `Cargo.lock` committed (binary project), which is
   the standard practice.
5. **Lint strictness**: warn or hard-deny for `unwrap_used`/ `expect_used`? And
   do you want `cargo fmt` enforced with a committed `rustfmt.toml` (vcard_tui
   has none — default fmt)?
6. **MSRV**: pin `rust-version` (io-imap generation wants 1.88) or omit until a
   dependency forces it?
7. **AGENTS.md**: vcard_tui carries an AGENTS.md for agent tooling; nitidus's
   standards live in `.claude/rules/`. Should the scaffold add an AGENTS.md that
   points at those rules (for non-Claude tooling), or skip it?
8. **Logging detail**: log file at `~/.local/state/nitidus/nitidus.log` with
   `RUST_LOG`-style env-filter (default `info`) — confirm, or prefer vcard_tui's
   cwd-relative log file?

### 3.2 R1 Answers

1. use nitidus
2. confirmed, also vcard_tui will most likely just be abandoned, it's really
   just a POC and should not be relied on as a model for designing this app.
3. dual license
4. yes, caret ranges at known-good minimums
5. hard-deny and yes enforce cargo fmt
6. omit
7. ignore vcard_tui as a model, don't import any files or code.
8. confirm

## 4. Plan

Phases 1–3 land as one commit (a members-globbed workspace does not compile
until at least one member exists); phase 4 proves it. The workspace
compiles and tests green at the end.

### Phase 1 — Workspace root

1. Root `Cargo.toml`:
   - `[workspace]`: `members = ["crates/*"]`, `resolver = "3"`.
   - `[workspace.package]`: `edition = "2024"`,
     `license = "MIT OR Apache-2.0"`,
     `repository = "https://github.com/kenianbei/nitidus"`. No
     `rust-version` (R1.6).
   - `[workspace.dependencies]`, caret ranges at known-good minimums
     (R1.4): bevy 0.18 (`default-features = false`, features `bevy_log`,
     `bevy_state`), bevy_ratatui 0.11, plurimus 0.1 (feature `ui`),
     ratatui 0.30 (features `crossterm`, `palette`,
     `unstable-widget-ref`), ratatui-image 11, tui-prompts 0.6,
     image 0.25, tokio 1 (`rt-multi-thread`), tokio-util 0.7, flume 0.12,
     serde 1 (`derive`), toml 1, thiserror 2, anyhow 1, tracing 0.1,
     tracing-subscriber 0.3 (`env-filter`), tracing-appender 0.2,
     etcetera 0.11, plus the four member crates by path.
   - `[workspace.lints.clippy]`: `unwrap_used = "deny"`,
     `expect_used = "deny"`.
2. `LICENSE-MIT` and `LICENSE-APACHE` at the root.
3. `rustfmt.toml`: `style_edition = "2024"`.
4. Confirm the fork's `.gitignore` covers `/target` (it does; keep as is).

### Phase 2 — Library crates

5. `crates/nitidus-mail`, `crates/nitidus-ui-kit`,
   `crates/nitidus-contacts`, each with:
   - `Cargo.toml` inheriting `*.workspace = true` for package fields and
     lints; version `0.1.0`; no dependencies yet.
   - `src/lib.rs`: crate-level doc comment stating the crate's single
     responsibility, a `pub fn crate_version() -> &'static str`
     placeholder, and a `#[cfg(test)]` smoke test module asserting the
     version string is non-empty (with the module-level allow for
     unwrap/expect).
   - `nitidus-mail/Cargo.toml` documents the no-bevy invariant where it
     is enforced: a manifest comment stating bevy must never appear in
     its dependency tree.

### Phase 3 — Binary crate

6. `crates/nitidus`: binary target `nitidus` (R1.1), thin per
   rules/testing.md §13:
   - `src/lib.rs`: `logging` module — `init()` resolves the XDG state dir
     via etcetera, creates it if missing, and installs
     tracing-subscriber with an `EnvFilter` (`RUST_LOG`, default `info`)
     writing to `nitidus.log` via tracing-appender (R1.8); plus `run()`
     which logs startup at info and returns `Ok(())`.
   - `src/main.rs`: `fn main() -> anyhow::Result<()>` calling
     `logging::init()` then `run()` — under 15 lines.
   - Dependencies: anyhow, tracing, tracing-subscriber, tracing-appender,
     etcetera (all `workspace = true`). Sibling crates are added when
     later items need them, not preemptively.

### Phase 4 — Verification

7. `cargo fmt --check`; `cargo clippy --workspace` (zero warnings);
   `CARGO_INCREMENTAL=0 cargo test --workspace` (green with nonzero pass
   counts); `cargo run` writes the log file under the XDG state dir and
   exits 0. Record results in §5, commit per contributing.md.

## 5. Verification

All run 2026-07-24 on rustc/cargo 1.93.1:

- `cargo fmt --check` — clean (after one mechanical `cargo fmt` fix in
  `logging.rs`).
- `cargo clippy --workspace` — finished with zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **5 passed, 0 failed**
  across the four crates: `nitidus` lib (`run_succeeds`,
  `state_dir_ends_with_app_name`), `nitidus-contacts`, `nitidus-mail`,
  `nitidus-ui-kit` (one `crate_version_is_nonempty` each).
- `cargo run` — exit 0; wrote
  `~/.local/state/nitidus/nitidus.log` containing
  `INFO nitidus: nitidus 0.1.0 started`.

## 6. Implementation Report

Implemented exactly as planned; no deviations from §4. Notes:

- `LICENSE-APACHE` is the canonical apache.org text (202 lines);
  `LICENSE-MIT` carries the 2026 copyright.
- The fork's `.gitignore` already covered `/target` and was kept
  unchanged.
- Unused `[workspace.dependencies]` entries (the bevy/UI stack) resolve
  nothing into `Cargo.lock` until a member crate references them — the
  scaffold's lock file stays small, and the heavy UI dependencies are
  first fetched when item 1a.2 lands.
- `etcetera::choose_base_strategy()` uses XDG on Linux; on macOS/Windows
  `state_dir()` is `None`, so logging falls back to the platform data
  dir — acceptable until a platform pass.
- Follow-up items: the no-bevy invariant for `nitidus-mail` is a
  documented manifest comment only — an automated `cargo tree` check
  belongs in the future CI design item; `README.md` still describes the
  upstream fork and needs a rewrite in its own chore.

## 7. Testing and Cleanup
