# feature - Index Customization - v1

Roadmap 1f.26: make the message index yours — configurable columns, row styling
driven by the theme instead of hardcoded modifiers, and configurable date
display. The phase 3 items (pattern-driven per-column colors, full conditional
formatting) stay out; this is the config-file tier.

## 1. Current Design

- **Columns are compiled in**: `index/render.rs` renders exactly flags(4) ·
  date(12) · from(30%, max 30) · subject(rest), in that order, as constants. No
  configuration surface exists.
- **Row styling is hardcoded**: selected/normal come from the theme's default
  states, marked rows use the info state (1f.25), the search highlight uses
  warning (1f.24) — but unseen = `BOLD` and deleted = `DIM` are raw modifiers
  applied in `row_style`, invisible to any theme.
- **Date display is conditional but fixed**: `format_date` renders `HH:MM`
  today, `Jul 24` this year, `2024-02-15` otherwise — reasonable defaults, zero
  configurability.
- **Config surface**: `[ui]` holds exactly one key, `theme`, validated against
  the single preset (`tailwind-dark`). The theme system itself is seed-color
  based (`ThemeColorStates::derive` builds
  normal/disabled/focused/hovered/selected from two seed colors), so new presets
  are cheap — but only one exists, and nothing about _rows_
  (unseen/flagged/deleted) lives in the theme.
- Config loading is strict (`deny_unknown_fields`, validation with friendly
  errors) — new `[ui.index]` keys get load-time validation for free.

## 2. Proposal

1. **`[ui.index] columns`**: an ordered list of column names —
   `["flags", "date", "from", "subject"]` by default; any subset/order. Widths
   stay built-in per column (subject is always the flexible filler); unknown
   names are load-time errors, and a layout without `subject` is allowed (your
   index, your loss). The thread indent and search highlight follow whichever
   columns render.
2. **Theme-driven row styling**: the theme gains an `index` role map — `unseen`,
   `flagged`, `deleted`, `marked` — each a palette-state reference resolved at
   theme build. The tailwind-dark preset maps them to today's look (unseen
   bold-bright, deleted dimmed, marked info), so nothing changes visually by
   default, but a theme now owns the entire row appearance and future
   presets/themes can restyle without code. Flagged rows gain a subtle warning
   tint they never had (currently only the `F` flag char distinguishes them) —
   the one deliberate visual change.
3. **`[ui.index] date`**: `"auto"` (today's three-tier behavior, default),
   `"time"` (always `HH:MM`), `"short"` (always `Jul 24`), `"iso"` (always
   `2024-02-15`). Enum, not strftime — custom format strings can join the phase
   3 conditional-formatting work.
4. Everything reloads the same way config already does (restart); hot-reload
   stays a phase 2 item.

Out of scope: pattern-driven per-column colors and conditional formats (phase
3), custom strftime strings, per-account column sets, column widths in config
(add later if the built-ins pinch), and additional theme presets (cheap now, but
a design pass of its own).

## 3. Discussion

### 3.1 R1 Questions

1. **Column config shape.** Names-only array with built-in widths, subject as
   the flexible filler, load-time validation. Or do you want widths in config
   now (`{ name = "from", width = 40 }` tables)?
2. **The flagged tint.** Giving flagged rows a warning-colored tint is the one
   visible default change (unseen/deleted/marked keep today's look, just
   theme-owned). Want it, or keep flagged rows plain until you restyle
   deliberately?
3. **Date options.** The four-value enum (`auto`/`time`/`short`/`iso`) cover
   you? Anything else you'd actually use?
4. **Columns available.** `flags`, `date`, `from`, `subject` are what the
   envelope carries today. A `size` column would need schema+backend work (IMAP
   RFC822.SIZE is fetchable but not stored) — propose leaving it out of v1 and
   noting it as a follow-up. OK?
5. **Smoke.** You drive: reorder columns in config, drop one, set
   `date = "iso"`, eyeball the flagged tint, restart between edits. OK?

### 3.2 R1 Answers

1. option a
2. tint
3. proposed
4. ok
5. ok

## 4. Plan

Each phase compiles, passes clippy, and keeps tests green.

### Phase 1 — config schema

- `config/schema.rs`: `UiConfig` gains `index: IndexUiConfig`
  (`#[serde(default, deny_unknown_fields)]`) with `columns: Vec<IndexColumn>`
  (default `[Flags, Date, From, Subject]`) and `date: DateFormat` (default
  `Auto`). `IndexColumn` (`flags|date|from|subject`) and `DateFormat`
  (`auto|time|short|iso`) are lowercase serde enums — an unknown name fails at
  parse time with the valid set listed, so no hand-rolled validation.
- `config/load.rs`: `validate` rejects duplicate columns (empty list is
  allowed — your index, your loss).
- `documentation/example-config.toml`: add a commented `[ui.index]` example.
- Tests: defaults round-trip, `[ui]` overlay leaves index defaults intact,
  unknown column and unknown date value produce naming errors, duplicate
  column rejected.

### Phase 2 — theme-owned row styling

- `nitidus-ui-kit/theme`: `Theme` gains `index: ThemeIndexStyles` with four
  `Style` patches — `unseen`, `flagged`, `deleted`, `marked` — composed over
  the row's base style at render. `tailwind_dark()` maps unseen → `BOLD`,
  deleted → `DIM`, marked → base info normal, flagged → warning-fg tint (the
  one visual change).
- `index/render.rs`: `RowStyles::from_theme` copies the role patches instead
  of hardcoding modifiers; `IndexRow` gains `flagged: bool`; `row_style`
  patches roles in a fixed order (marked/selected base, then unseen, flagged,
  deleted) so precedence is deterministic.
- Tests: preset roles reproduce today's unseen/deleted/marked styles; flagged
  row style differs from normal; selected+flagged keeps the selected bg.

### Phase 3 — column order and date format

- `index/render.rs`: replace the inline four-column layout with a width
  resolution over the configured column list (built-in widths: flags 4,
  date 12, from 30 %/max 30; subject absorbs the remainder; the last column
  always fills to pane width). `format_date` takes the `DateFormat` (`Auto`
  keeps the three-tier behavior). `IndexWindowState` carries the resolved
  columns and date mode the way it already carries `search`; if the render
  file crosses the 300-line budget, the layout resolution splits into
  `index/columns.rs`.
- `index/mod.rs`: the window build reads `Res<Config>` (already available to
  the plugin) and passes `ui.index` through.
- Tests: default order renders byte-identical to today's layout, a reordered
  and a subset layout place fields where configured, layout without subject
  still fills the width, each date mode formats the same timestamp correctly.

### Phase 4 — docs and verification

- Update `documentation/specification.md` config section with `[ui.index]`.
- `cargo clippy --workspace --all-targets`,
  `CARGO_INCREMENTAL=0 cargo test --workspace` with pass counts, then the
  user-driven smoke from R1 #5.

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **413 passed, 0 failed**
  (404 before this feature; the delta is the new config, preset, and
  render tests plus the extended overlay test).
- Default-layout invariance: `default_columns_fill_exact_width_in_the_established_order`
  asserts the default column set renders the same leading bytes and exact
  pane width as the pre-change layout; the subject-width arithmetic
  reproduces the old `FLAGS + DATE + from + 3` computation for the
  four-column default.
- User smoke (R1 #5): passed — `columns = ["date", "subject", "from"]`
  with `date = "iso"` renders as configured; flagged tint confirmed. One
  expectation surfaced: no column *headers* exist (never in scope) —
  noted as a possible follow-up alongside refactor-ui-v1.

## 6. Implementation Report

All four phases landed as planned, with two small deviations:

- **Serde does the column validation.** `columns` deserializes straight
  into an `IndexColumn` enum (`rename_all = "lowercase"`), so an unknown
  name fails at parse time with the valid set listed — no hand-rolled
  validation. Only the duplicate check lives in `validate` (empty lists
  are allowed). `DateFormat` works the same way.
- **Param-limit refactors while touching.** `build_row` and `row_line`
  would have crossed 4 parameters, so per-window inputs grouped into
  `RowBuildContext` (now/date/selected/marked) and `RowContext`
  (styles + columns, carried by `IndexWindowState` in place of the bare
  `RowStyles`). `build_window_state` likewise takes a `WindowSource`
  struct, with the row-window loop split into `build_window_rows`.
  `refresh_index` gained `Res<Config>` and went to tuple-grouped system
  params to stay under clippy's argument limit.

Details worth recording:

- `ThemeIndexStyles` (nitidus-ui-kit) holds four `Style` patches —
  `unseen`, `flagged`, `deleted`, `marked` — composed over the row's base
  in a fixed order (base by selection state, then unseen, flagged,
  deleted), so precedence is deterministic. The flagged tint patches fg
  only; a selected flagged row keeps the selected background.
- The old three-tier date logic became `auto_pattern`; forced modes reuse
  the same pattern constants.
- The trailing whole-line `fit` pads to pane width, so a layout without
  `subject` still fills the row — the "last column stretches" behavior
  falls out for free.
- `RowContext::default()` uses the default column order (not an empty
  list), so a widget rendered before the first refresh behaves like
  today's layout.
- `documentation/specification.md` already lists "Configurable index
  columns and theme-driven row styling" as a core feature; key-level docs
  went into `documentation/example-config.toml` (`[ui.index]` block),
  which the schema test keeps parseable.

Follow-ups (unchanged from the proposal): `size` column (needs
schema+backend storage of RFC822.SIZE), config-file widths, strftime
strings, pattern-driven colors (phase 3), additional theme presets.

## 7. Testing and Cleanup

Comment pass over the branch diff: all new comments state invariants
(patch precedence order, the stretch-to-width behavior of the trailing
fit, why `RowContext::default` carries real columns); none removed.
No dead code — clippy reports zero warnings across the workspace.
Final verify: `cargo fmt --all` clean,
`cargo clippy --workspace --all-targets` zero warnings,
`CARGO_INCREMENTAL=0 cargo test --workspace` **413 passed, 0 failed**.
