# feature - app shell - v1

Boot nitidus into a real terminal UI for the first time: a bevy app wired to
bevy_ratatui/plurimus, a theme resource, the root layout, a statusline, and a
tab-bar shell. This is roadmap item 1a.2 — after this lands, `cargo run` shows
persistent application chrome instead of logging and exiting, and every later
screen (index, pager, compose, contacts) has a frame to land in.

## 1. Current Design

From item 1a.1 (`design/feature-workspace-scaffold-v1.md`, merged):

- `crates/nitidus/src/main.rs` initializes file logging and calls
  `nitidus::run()`, which logs a startup line and returns — no UI, no event
  loop.
- `crates/nitidus-ui-kit` is an empty placeholder crate with no dependencies.
- The workspace pins the UI stack in `[workspace.dependencies]` (bevy 0.18
  `default-features = false` + `bevy_log`/`bevy_state`, bevy_ratatui 0.11,
  plurimus 0.1 feature `ui`, ratatui 0.30, ratatui-image 11, tui-prompts 0.6,
  image 0.25) but no member crate references any of it yet — this item pulls the
  stack into the build for the first time.

Verified against docs.rs for plurimus 0.1.0: it exports `PlurimusPlugin`, widget
components `Widget`, `WidgetLayout`, `WidgetOrder`, `WidgetRect`, draw traits
`DrawArea`, `DrawFn`, `DrawOrder`, and a `PlurimusFixedSet` schedule set; the
`ui` feature adds focus/interaction components and systems. Widgets are ECS
entities; rendering goes through bevy_ratatui's `RatatuiContext`.

vcard_tui is a design/version reference only (R1 of the scaffold doc): it
demonstrates this stack working (MinimalPlugins + ScheduleRunnerPlugin at
60fps + RatatuiPlugins + StatesPlugin + PlurimusPlugin) but no code is imported
from it.

## 2. Proposal

Two crates gain code:

### `nitidus-ui-kit` (takes its first dependencies: bevy, ratatui, plurimus)

- **`theme` module** — a fresh seed-derived theme system:
  - `ThemeColor` (RGB newtype) with `darken(f32)` / `lighten(f32)`.
  - `ThemeColors { bg, fg }` per interaction state;
    `ThemeColorStates { normal, disabled, focused, hovered, selected }` derived
    from a single seed pair via darken/lighten rules.
  - `ThemePalette { default, error, info, success, warning }` and
    `Theme { base, paper }` (app chrome vs popup surfaces), registered as a bevy
    `Resource`.
  - One built-in preset: a dark theme seeded from ratatui's Tailwind palette.
    Conversions to ratatui `Style`/`Color`.
- **`layout` module** — `LayoutFn` (`Arc<dyn Fn(Rect) -> Rect>`) plus a
  root-frame splitter producing the shell regions (tab-bar row, content area,
  statusline row) as indexed layout closures widgets can own.

### `crates/nitidus` (bin)

- **`app` module** — `build_app() -> App`: MinimalPlugins +
  `ScheduleRunnerPlugin::run_loop(1/60s)`, bevy_ratatui's `RatatuiPlugins`,
  `StatesPlugin`, `PlurimusPlugin`, the `Theme` resource, and the shell plugin.
  `run()` switches from log-and-exit to `build_app().run()`.
- **`shell` plugin** — spawns the persistent chrome as plurimus widget entities:
  - **Tab bar** (single row): renders tab chips from a `Tabs` resource; ships
    with one placeholder tab ("mail") — real tab machinery arrives with later
    items.
  - **Statusline** (single row): left segment = active tab name; right segment =
    `nitidus v{version}`; styled from `Theme`; later items extend it (connection
    state, keychord hints).
  - **Content pane**: an empty themed block filling the content area — the mount
    point future screens replace.
- **Quit placeholder**: a global key binding (`q` and `Ctrl-C`) sending
  `AppExit`, explicitly temporary until the action router (item 1a.4) owns all
  input.

### Testing strategy

Pure logic gets unit tests: theme derivation (darken/lighten math, state
derivation from seeds), layout math (region splits at various terminal sizes),
statusline segment assembly. Widget-spawning systems get headless bevy
`App::update()` tests where possible without a real terminal; full-terminal
behavior is verified manually via `cargo run` (documented in §5).

Out of scope: action router and command line (1a.4), config-driven theming
(needs 1a.3), multiple/dynamic tabs, draw-skip idle optimization and custom
park-on-event runner (recorded follow-ups), any mail content.

## 3. Discussion

### 3.1 R1 Questions

1. **Chrome placement**: proposal puts the tab bar on the top row and the
   statusline on the bottom row (aerc-style). vcard_tui instead used a 2-row
   bottom toolbar. Confirm top-tabs/bottom-status?
2. **Tick rate**: 60fps (`run_loop(1/60)`, matches vcard_tui, smooth for future
   tachyonfx effects) or 30fps (halves idle wakeups; battery)?
   Draw-skip/custom-runner work is deferred either way.
3. **Theme depth now**: build the full structure (base+paper surfaces × 5
   palettes × 5 interaction states, one dark preset) as proposed, or a minimal
   single-palette version grown on demand? The full structure front-loads API
   shape so later items don't churn callers.
4. **Terminal features**: enable the kitty keyboard-enhancement protocol and
   mouse capture from day one (both supported by RatatuiPlugins)?
5. **Quit placeholder**: `q` + `Ctrl-C` acceptable until 1a.4 replaces them?
   (`q` will eventually be a real keymap binding; `Ctrl-C` stays as the
   emergency exit.)
6. **ui-kit dependency step**: confirm `nitidus-ui-kit` takes
   bevy/ratatui/plurimus dependencies now. The alternative — keeping ui-kit
   dependency-free and hosting theme/layout in the bin crate — preserves a
   lighter crate but guarantees a later extraction refactor.

### 3.2 R1 Answers

1. Confirm, follow aerc-style.
2. 30fps is fine.
3. full structure
4. use plurimus ui & tachyonfx features, and make full use of them for UI state
   and effects.
5. yes
6. plurimus as dep

## 4. Plan

Interpreting R1.4: the workspace plurimus dependency gains the
`tachyonfx` feature alongside `ui`; mouse capture and the kitty keyboard
protocol are enabled in `RatatuiPlugins` because hover/press interaction
state requires mouse events and enhanced key handling. Each phase leaves
the workspace compiling with clippy and tests green.

### Phase 1 — ui-kit theme module

1. Workspace: extend plurimus features to `["ui", "tachyonfx"]`.
2. `nitidus-ui-kit` dependencies: bevy, ratatui, plurimus (workspace).
3. `theme/` module (split by responsibility, files ≤300 lines):
   - `color.rs` — `ThemeColor` RGB newtype, `darken`/`lighten`,
     `From<ThemeColor> for ratatui::style::Color`.
   - `states.rs` — `ThemeColors { bg, fg }` (+ `style()`),
     `ThemeColorStates { normal, disabled, focused, hovered, selected }`
     with `derive_from_seed(bg, fg)` encoding the darken/lighten rules.
   - `palette.rs` — `ThemePalette { default, error, info, success,
     warning }`, `Theme { base, paper }` deriving bevy `Resource`.
   - `presets.rs` — `tailwind_dark()` seeded from ratatui's Tailwind
     palette (slate surfaces, amber focus/selection accents).
4. Unit tests: darken/lighten bounds and monotonicity, state derivation
   from seeds, ratatui conversions, preset sanity (distinct states).

### Phase 2 — ui-kit layout module

5. `layout.rs`: `LayoutFn` (`Arc<dyn Fn(Rect) -> Rect>`),
   `ShellRegions { tab_bar, content, statusline }` computed by a
   `split_shell(Rect)` (top row / fill / bottom row), and
   `shell_layout_fns()` returning per-region `LayoutFn`s for widgets.
6. Unit tests: splits at normal sizes, degenerate terminals (height 0–2
   collapse gracefully, no panics or overlaps).

### Phase 3 — bin app + shell plugin

7. Bin dependencies: bevy, bevy_ratatui, plurimus, ratatui,
   nitidus-ui-kit.
8. `app.rs` — `build_app() -> App`: MinimalPlugins +
   `ScheduleRunnerPlugin::run_loop(1/30s)` (R1.2), `RatatuiPlugins` with
   kitty protocol + mouse capture enabled, `StatesPlugin`,
   `PlurimusPlugin`, `Theme` resource (`tailwind_dark()`), `ShellPlugin`.
   `run()` becomes `build_app().run()` returning `Ok(())` after exit.
9. `shell.rs` — `ShellPlugin`:
   - `Tabs` resource (labels + active index), initialized with the
     placeholder "mail" tab.
   - Startup systems spawning three plurimus widget entities (tab bar,
     content pane, statusline) with ui-kit layout fns and theme-driven
     draw fns; `on_change` systems gated on `Tabs`/`Theme`
     `is_changed()` refresh widget content.
   - Statusline: left = active tab name, right = `nitidus v{version}`.
   - A subtle tachyonfx fade-in effect on the shell at startup, proving
     the effects pipeline end-to-end (R1.4).
   - Quit placeholder: global bindings `q` and `Ctrl-C` → `AppExit`,
     marked for replacement in 1a.4.
10. Headless tests: a bevy `App` with `ShellPlugin` (and stub resources,
    without RatatuiPlugins) updates once and asserts the three widget
    entities exist and statusline text assembles correctly; pure
    assembly helpers unit-tested directly where headless ECS setup is
    impractical.

### Phase 4 — Verification

11. `cargo fmt --check`; `cargo clippy --workspace` (zero warnings);
    `CARGO_INCREMENTAL=0 cargo test --workspace` (green, pass counts up
    from the scaffold's 5); manual `cargo run`: chrome renders (top tab
    bar, themed content pane, statusline with version), `q` and `Ctrl-C`
    both exit cleanly, terminal restored, log file written. Record in
    §5, commit per contributing.md.

## 5. Verification

All run 2026-07-24 on rustc/cargo 1.93.1:

- `cargo fmt --check` — clean.
- `cargo clippy --workspace` — zero warnings (first full build of the
  bevy 0.18 + bevy_ratatui 0.11 + plurimus 0.1 stack).
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **22 passed, 0 failed**:
  nitidus 5 (logging state dir, statusline state via headless ECS app,
  three-widget spawn, statusline padding, default tabs), nitidus-ui-kit
  15 (theme color math, state derivation, preset sanity, layout splits
  incl. degenerate heights), nitidus-mail 1, nitidus-contacts 1.
- Terminal run (pty via `script` at 80×24, this machine has no
  interactive TTY): app boots, renders the tab chip `mail` and
  statusline `mail … nitidus v0.1.0`; **`q` exits 0** and **`Ctrl-C`
  exits 0**; terminal restored; log shows
  `nitidus 0.1.0 starting` / `nitidus exited cleanly`.

## 6. Implementation Report

Implemented per §4 with these notes:

- plurimus already exports the `LayoutFn` type alias and `WidgetLayout` —
  ui-kit's layout module builds on plurimus's type instead of defining
  its own (plan §4 assumed a local alias; using upstream's is strictly
  better).
- `PlurimusPlugin` hardcodes `Time::<Fixed>` to 1/16s for its
  layout+draw schedule; `build_app` overrides it to 1/30s alongside the
  30fps runner so drawing actually happens at the configured rate.
- Widget refresh pattern: `Res::is_changed()` fires on first run, so the
  same `refresh_*` systems handle both initial population and later
  updates — no separate init path.
- The statusline is a `from_render_fn_with_state` widget (state =
  left/right strings + style; render pads to width at draw time); the
  tab bar and content pane are `set_widget`-replaced Paragraph/Block
  widgets. All three builders are pure functions, unit-tested headless.
- The tachyonfx `coalesce` fade-in runs on the content pane at startup
  (`enable_fx`/`add_fx` against the NonSend `TachyonRegistry`).
- Verification gotcha worth remembering: a pty created by `script` in a
  headless environment has a 0×0 winsize — plurimus computes zero-area
  rects and renders nothing. `stty rows 24 cols 80` inside the pty fixes
  it; this cost a debugging round and belongs in any future headless
  TUI-testing recipe.
- Follow-ups: draw-skip/idle optimization and a park-on-event runner
  remain deferred (recorded in ratatui-frameworks.md); `q` binding moves
  into the keymap system in item 1a.4; tab machinery is a placeholder
  single tab until real tabs arrive.

## 7. Testing and Cleanup
