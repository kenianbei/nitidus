# feature - config loading - v1

Load user configuration at startup: XDG-resolved `config.toml` (accounts, app
settings) and `keys.toml` (keybindings), parsed strictly against typed schemas
with compiled-in defaults, exposed to the app as resources. This is roadmap item
1a.3 — it gives the action router (1a.4) its keymap source, the account wizard
(1d.20) its write target, and the mail engine (1a.5+) its account definitions.
No secrets ever appear in config files.

## 1. Current Design

- `crates/nitidus/src/logging.rs` already resolves the XDG **state** dir via
  `etcetera::choose_base_strategy()` (with a data-dir fallback for macOS/Windows
  where `state_dir()` is `None`) — the same pattern this item needs for the
  **config** dir.
- No config code exists anywhere; `serde` (derive) and `toml` 1.x are pinned in
  `[workspace.dependencies]` but unused. `crokey` (key-notation parsing) is
  recommended in documentation/rust-libraries.md but not yet a workspace
  dependency.
- `documentation/persistence.md` §2 and §8 spec the contract this item
  implements: `~/.config/nitidus/{config.toml,keys.toml}`; **strict parsing for
  config** (typos error loudly, `deny_unknown_fields`) vs lenient for state
  files; all config optional — compiled-in defaults are a complete working
  setup; no plaintext secrets, only references (credential commands, keyring).
- `documentation/specification.md` requires: multiple accounts (identity,
  aliases, folder mapping, signatures per account), OAuth2/credential- command
  auth references, TOML config + keybinding files.
- The shell (1a.2) inserts `Theme` and `Tabs` resources directly; nothing reads
  user preferences yet. The temporary `q` quit binding is hardcoded pending
  1a.4, which will compile keymaps from what this item parses.

## 2. Proposal

A `config` module in the bin crate (`crates/nitidus/src/config/`), split by
responsibility:

- **`dirs.rs`** — `resolve_config_dir()` (etcetera, `XDG_CONFIG_HOME` honored;
  macOS/Windows platform equivalents), shared constants for file names. Refactor
  opportunity: extract the strategy-resolution helper shared with `logging.rs`.
- **`schema.rs`** — the typed model, all `#[serde(deny_unknown_fields)]` with
  `#[serde(default)]` per field (field-level merge over defaults):
  - `Config { accounts: Vec<AccountConfig>, ui: UiConfig }`
  - `AccountConfig` — `name` (unique id), `email`, `display_name`, `backend`
    (tagged enum: `maildir { path }` | `imap { host, port, encryption }`),
    `outgoing` (tagged enum: `smtp { host, port, encryption }` |
    `sendmail { command }`), `auth` (`password_cmd` | `keyring` |
    `oauth2 { provider }` — references only, never secret material), `folders`
    (drafts/sent/trash/archive name mapping), `signature` (inline string or file
    path). Parsed and validated now; consumed when the engine/backends land.
  - `UiConfig` — minimal on purpose: `theme` (preset name, currently only
    `"tailwind-dark"`) as the single field, proving the section exists without
    inventing unused options.
  - `RawKeymaps` — `BTreeMap<context, BTreeMap<key-sequence, command>>`
    mirroring keys.toml (`[index] "dd" = ":delete"`). Key sequences are
    **syntax-validated at load** (crokey) so bad notation fails at startup, but
    binding compilation/semantics stay in 1a.4.
- **`load.rs`** — `load() -> anyhow::Result<LoadedConfig>`: missing files →
  defaults (info log); present files → strict parse with errors carrying file
  path and serde's line/column context. Duplicate account names and dangling
  signature paths are validation errors.
- **Startup wiring** — `main` loads config **before** the TUI starts; a
  parse/validation error prints a friendly message to stderr and exits nonzero
  (never a half-started TUI). `run()` takes the loaded config; `build_app()`
  inserts `Res<Config>`-style resources (`Config` + `RawKeymaps`) and selects
  the theme preset by name.
- **Defaults as documentation**: `Config::default()` round-trips through toml to
  generate a commented example config in `documentation/` (checked by a test
  that the example stays parseable).

New workspace dependency: `crokey` (already vetted in rust-libraries.md). Bin
crate gains `serde`, `toml`, `crokey`.

### Testing strategy

Unit tests against string fixtures (no filesystem): defaults are complete and
valid; field-level overlay works; unknown fields error with the offending key;
each backend/auth/outgoing variant parses; duplicate account names rejected; bad
key notation rejected with context; missing files yield defaults. One
integration-style test drives `load()` against a temp dir (`tempfile` already in
workspace) for the missing/present/ malformed file paths.

Out of scope: keymap compilation and the action router (1a.4), the account
wizard (1d.20), config hot-reload (Phase 2), theme definitions in config beyond
preset selection (Phase 2 index customization), any use of `AccountConfig` by an
actual backend (1b.6/1b.12), CLI argument parsing.

## 3. Discussion

### 3.1 R1 Questions

1. **Account schema breadth now**: the proposal parses the full account shape
   (backend, outgoing, auth, folders, signature) even though nothing consumes it
   until 1b — users can write real configs early and the wizard target is fixed.
   Alternative: accounts as name+email only now, grown per item. Full shape now?
2. **Startup failure mode**: on malformed config, exit nonzero with a stderr
   message before the TUI starts (proposed), or start with defaults and surface
   the error in the statusline? Exit-early is safer (a mail client silently
   running on defaults could surprise); statusline-surfacing needs 1a.4+ anyway.
3. **keys.toml validation depth**: validate key-sequence syntax with crokey at
   load (proposed, adds the dep now), or treat keys.toml as opaque strings until
   1a.4 compiles them? Early validation gives errors at the moment the user
   edits the file.
4. **Env override**: honor a `NITIDUS_CONFIG_DIR` env var (useful for testing
   and portable setups) in addition to XDG, or XDG only until a real need
   appears?
5. **Example config**: generate `documentation/example-config.md` (or
   `config.example.toml` at repo root?) from defaults with a parseability test —
   worth it now, or defer to the wizard item?
6. **`ui.theme` field**: include the single-field `[ui]` section now as
   proposed, or ship zero UI options until theming config actually grows?
   (Including it exercises the section-merge machinery early.)

### 3.2 R1 Answers

1. full shape now
2. exit
3. validate at load
4. honor `NITIDUS_CONFIG_DIR`
5. yes
6. yes

## 4. Plan

Each phase leaves the workspace compiling with clippy and tests green.

### Phase 1 — Shared XDG dirs + dependencies

1. Workspace: add `crokey = "1.4"`; bin crate gains `serde`, `toml`,
   `crokey`.
2. New `crates/nitidus/src/dirs.rs`: shared base-strategy resolution —
   `state_dir()` (moved from logging.rs, behavior unchanged) and
   `config_dir()` (XDG config dir, `NITIDUS_CONFIG_DIR` env override
   taking precedence). `logging.rs` refactored onto it.

### Phase 2 — Schema

3. `config/account.rs`: `AccountConfig` (name, email, display_name,
   aliases), `Backend` (externally-tagged enum: `maildir { path }`,
   `imap { host, port [993], encryption [tls|starttls|none, default
   tls] }`), `Outgoing` (`smtp { host, port [587], encryption }`,
   `sendmail { command }`), `Auth` (`password_cmd { command }`,
   `keyring`, `oauth2 { provider [google|microsoft] }`), `Folders`
   (drafts/sent/trash/archive with conventional defaults), signature as
   mutually-exclusive `signature` / `signature_file`. All structs
   `#[serde(default, deny_unknown_fields)]`.
4. `config/schema.rs`: `Config { accounts, ui }`, `UiConfig { theme }`
   (default `"tailwind-dark"`); bevy `Resource` derives on `Config` and
   `RawKeymaps`.
5. `config/keymaps.rs`: `RawKeymaps` (context → sequence → command) and
   `parse_key_sequence()` — tokenizes aerc-style notation (bare chars,
   `<...>` groups with `C-`/`A-`/`S-` modifiers and named specials) into
   crokey `KeyCombination`s for load-time validation; 1a.4 reuses the
   same parser for compilation.

### Phase 3 — Loader + wiring

6. `config/load.rs`: `load()` → `LoadedConfig { config, keymaps }`;
   missing files → defaults (info log); parse errors carry file path +
   toml's span context; validation: duplicate account names, both
   signature forms set, dangling `signature_file`, unknown theme name,
   invalid key sequences (with context/sequence named).
7. Startup: `main` loads after logging init; on error prints
   `nitidus: {err:#}` to stderr and exits 1 — the TUI never half-starts.
   `run(LoadedConfig)` → `build_app(LoadedConfig)` inserts `Config` and
   `RawKeymaps` resources and selects the theme preset by name.
8. `documentation/example-config.toml`: hand-written commented example
   (one full account, keymap samples); a test parses it with the real
   schema so it can never rot.

### Phase 4 — Verification

9. fmt/clippy/full test suite; pty run with `NITIDUS_CONFIG_DIR`
   pointing at a temp dir holding the example config (app boots, theme
   applies); malformed-config run exits nonzero with a readable stderr
   message and no TUI. Record in §5, commit per contributing.md.

## 5. Verification

All run 2026-07-24 on rustc/cargo 1.93.1:

- `cargo fmt --check` — clean; `cargo clippy --workspace` — zero
  warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **44 passed, 0 failed**
  (27 in the bin crate: dirs, account schema variants, strict-typo
  rejection naming the offending key, key-sequence parsing incl.
  modifiers and malformed cases, loader validation incl. the temp-dir
  missing/present/malformed integration test, example-config
  parseability; 15 ui-kit; 1 + 1 mail/contacts).
- pty run with `NITIDUS_CONFIG_DIR` pointing at a dir containing
  `documentation/example-config.toml` + a keys.toml: app boots, renders
  chrome, `q` exits 0.
- Malformed-config run: exits **1** with
  `nitidus: failed to parse …/config.toml: TOML parse error at line 1,
  column 1` (offending line quoted) and no TUI startup.

## 6. Implementation Report

Implemented per §4 with these notes:

- The shared XDG helper landed as `crates/nitidus/src/dirs.rs`
  (`state_dir()` + `config_dir()` with the `NITIDUS_CONFIG_DIR`
  override); `logging.rs` shrank to pure logging concerns.
- Enum layout: externally-tagged serde enums
  (`backend = { imap = { … } }`) rather than internally-tagged
  (`type = "imap"`) — `deny_unknown_fields` is unreliable inside
  internally-tagged variants (serde buffers the content), and the
  external form keeps strictness working per variant.
- Key-notation support: bare chars, `<C-x>`/`<A-x>`/`<S-x>` (stackable),
  named specials (Enter/Tab/Esc/Space/arrows/PgUp/PgDn/Home/End/Del/F*),
  normalized onto crokey's grammar. The parser is exported
  (`config::parse_key_sequence`) for 1a.4 to reuse in trie compilation.
- The env-var integration test runs all three loader cases (missing /
  present / malformed) inside a single `#[test]` because `set_var` is
  process-global — noted with SAFETY comments; parallel tests never
  observe the variable mid-change.
- `main` now returns `ExitCode` and routes all three failure surfaces
  (logging init, config load, app run) through one `fail()` that prints
  `nitidus: {err:#}` — the anyhow context chain reads naturally
  (file path → toml span → offending line).
- Follow-ups: `RawKeymaps` defaults are empty — compiled-in default
  bindings arrive with the action router (1a.4), which also consumes
  `parse_key_sequence`; `AccountConfig` is parsed-but-unconsumed until
  the engine items (1a.5, 1b.6, 1b.12); `~` in paths is not expanded yet
  (needs a decision when maildir backend lands).

## 7. Testing and Cleanup
