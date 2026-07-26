# feature - Comfort Features - v1

Roadmap 1f.27, the last phase-1 item: the small triage-comfort verbs and polish
that make daily driving pleasant — an archive verb, mark-read delay (peek),
auto-advance, the `:help keys` table, and a first mouse pass.

## 1. Current Design

- **Mark-read is instant**: `pager::ops::open_selected` sets `SEEN`
  synchronously before the fetch is even dispatched. A half-second glance marks
  a message read; there is no peek.
- **Archive does not exist**: `folders.archive` is in the account config
  (defaults to `Archive`; the wizard's Gmail preset maps it to
  `[Gmail]/All Mail`) but nothing consumes it — no `:archive` command, no key.
  `:move` exists and is batch-aware and staged (z-undo).
- **No advance-after-action**: index delete/move removes rows, and the selection
  clamp happens to land on the next row — advance for free. Deleting from the
  pager closes it back to the index (`index/remove.rs`); flag ops (`u`, `*`)
  leave the cursor in place.
- **Help is already live**: `?` opens a searchable picker over the actual
  resolved bindings (active context + unshadowed globals; `Tab` flips to all
  contexts), and Enter executes the row — help doubles as a command palette. The
  roadmap's "`:help keys` live table" predates this (shipped with
  refactor-keymap-v1).
- **Mouse is captured but unread**: `enable_mouse_capture: true` in `app.rs`,
  and plurimus already provides the whole pipeline — `UiEvent::Mouse`,
  per-widget hit-testing with hover/focus/press state, mouse bindings and
  passthroughs. Nothing in nitidus consumes any of it. ratatui-comfy-tabs
  exposes an abstract u16 mouse API (hover + click) awaiting wiring;
  ratatui-explorer's `handle` accepts mouse input too. The theme's `hovered`
  state exists and is unused by these surfaces.

## 2. Proposal

1. **Archive verb**: `a` in index and pager → `:archive` — a move to the
   account's `folders.archive`, batch-aware (marks + visual range), staged with
   the same z-undo window as delete/move. Unknown archive folder is a status
   error, not a crash.
2. **Peek (mark-read delay)**: `[ui.pager] mark_read` controls when an opened
   message gains `SEEN`. Opening starts a timer; closing (or moving to another
   message) before it fires leaves the message unread. Default stays today's
   mark-on-open.
3. **Auto-advance**: after a destructive verb from the pager (delete/archive),
   open the next message instead of falling back to the index; a config default
   with a runtime toggle. In the index itself, row removal already advances —
   flag ops optionally join in (R1).
4. **`:help keys`**: declare the roadmap item shipped by refactor-keymap-v1;
   optionally add `:help` argument aliases if wanted.
5. **Mouse pass**: wire the existing plumbing — click selects (sidebar row,
   index row, contacts row, picker/explorer row; a click on the already-selected
   index row opens it), the comfy-tabs bar switches tabs on click, and the wheel
   scrolls index / pager / sidebar / pickers. Hover styling and drag
   interactions stay out.

Out of scope: which-key hints and leader menus (phase 2), snooze/mute and
post-triage advance policies (phase 2), hover restyling of rows, mouse in the
compose editor, terminal selection/paste behavior changes.

## 3. Discussion

### 3.1 R1 Questions

1. **Archive key and semantics.** `a` is free in both index and pager contexts.
   Archive = staged move to `folders.archive` (for Gmail that is
   `[Gmail]/All Mail`, which is Gmail's real archive semantics). OK?
2. **Peek config shape.** Proposal: `[ui.pager] mark_read` as `"open"` (default,
   today's behavior) | `"never"` (only `u` toggles) | a number of seconds (e.g.
   `1.5`) for the delay. A string-or-number TOML value costs a custom
   deserializer but reads naturally. Or would you rather a plain
   `mark_read_secs` number with `-1`-style sentinels avoided via a second bool
   key? And what do you actually want as _your_ default — instant, or a short
   peek?
3. **Auto-advance scope.** Confirm: pager delete/archive → next message (falling
   back to close when none), config default in `[ui.pager]`, plus a
   `:toggle-advance` runtime toggle? And should index flag ops (`u`, `*`)
   advance the cursor like `<Space>` does, or stay put?
4. **Help.** Anything you still want beyond the `?` picker — e.g. `:help keys`
   as a command-line alias — or do we mark the item done?
5. **Mouse surfaces.** The proposed v1 set: index (click select / click-open on
   selected, wheel), sidebar (click select with Enter semantics, wheel), tab bar
   (click), pager (wheel), pickers + explorer (click select, wheel). Anything to
   add or drop? Hover styling really out?
6. **Smoke.** Mouse needs a real terminal, so you drive: click/wheel through the
   surfaces, archive a message and z-undo it, peek a message under the delay,
   watch auto-advance after a pager delete. OK?

### 3.2 R1 Answers

1. ok
2. ok, instant as default
3. all three advance
4. Let's make the overlay larger, mainly in height, otherwise can mark done
5. Let's include hover styling. everything else is good as proposed.
6. ok

## 4. Plan

Each phase compiles, passes clippy, and keeps tests green.

### Phase 1 — `[ui.pager]` config

- `config/schema.rs`: `UiConfig` gains `pager: PagerUiConfig` with
  `mark_read: MarkRead` and `advance: bool` (default `true`). `MarkRead` is a
  custom-serde value: `"open"` (default) | `"never"` | a positive number of
  seconds; anything else fails at load naming the accepted forms. Serialize
  round-trips the same three shapes.
- `documentation/example-config.toml`: `[ui.pager]` block.
- Tests: all three shapes parse, negative/zero seconds and unknown strings are
  rejected with naming errors, defaults round-trip.

### Phase 2 — archive verb

- `action.rs`: `Action::Archive` / `:archive`; dispatch resolves the active
  account's `folders.archive` from `Config` and reuses the existing staged
  move machinery (batch-aware via `batch_ids`, z-undo, pager-aware through the
  same `was_in_pager` path as delete/move). Unresolvable folder → status
  error.
- `keymap/defaults.rs`: `a` → `:archive` in index and pager.
- Tests: single and batch archive stage a move to the configured folder;
  unknown archive folder errors without staging; help/keymap layout tests
  updated for the new binding.

### Phase 3 — peek and auto-advance

- `pager/ops.rs` + a `PeekTimer` resource: `open_selected` flags SEEN
  immediately only for `mark_read = "open"`; a delay arms the timer
  (envelope id + due time), a tick system fires it while that message is
  still open, and close/next-message/prev-message disarm or re-arm it.
  `"never"` leaves flagging to `u`.
- Auto-advance: the `was_in_pager` branch of `index/remove.rs` opens the row
  that lands in the removed row's position (next message) instead of closing,
  when `ui.pager.advance` is on and a row remains; falls back to close.
  `:toggle-advance` flips the resource at runtime with statusline feedback.
- Index flag ops: single-target `:toggle-read` / `:toggle-flag` advance the
  cursor one row (batch forms keep the cursor).
- Tests: peek flags after the delay and not before, close before due leaves
  unread, pager delete/archive advances and falls back to close on the last
  message, flag ops advance, `:toggle-advance` flips behavior.

### Phase 4 — taller help overlay

- `overlay/mod.rs`: replace the fixed `PANEL_MAX_HEIGHT = 16` cap with a
  proportional one — the picker may grow to the content area's height minus a
  small margin (constant), so `?` shows a real table on tall terminals while
  short terminals keep the clamp. Mark the roadmap item done.
- Tests: panel-height math at short and tall areas.

### Phase 5 — mouse pass

- Route `UiEvent::Mouse` passthroughs on the existing widgets (plurimus
  already hit-tests and delivers per-widget coordinates):
  - **Index**: click selects the row under the cursor (window-top offset
    math), click on the already-selected row opens it, wheel moves the
    cursor.
  - **Sidebar**: click selects with Enter semantics (`sidebar::select`),
    wheel moves.
  - **Tab bar**: wire ratatui-comfy-tabs' u16 mouse API — click switches
    tabs, hover uses its built-in hover state.
  - **Pager**: wheel scrolls.
  - **Pickers + explorer**: click selects a row, wheel scrolls.
- Hover styling: track the hovered row from mouse-move passthroughs and
  render it with the theme's `hovered` state in index, sidebar, and picker
  rows (tab bar comes free from comfy-tabs). Compose editor and drag stay
  out.
- Tests: coordinate→row mapping and click/wheel handlers driven by
  synthesized `UiEvent::Mouse` in the existing harnesses; hover style
  selection unit-tested like the row styles.

### Phase 6 — docs and verification

- `documentation/example-config.toml` already updated in phase 1; roadmap
  1f.27 complete closes phase 1f.
- `cargo clippy --workspace --all-targets`,
  `CARGO_INCREMENTAL=0 cargo test --workspace` with pass counts, then the
  user-driven smoke from R1 #6 (real terminal: clicks, wheel, hover, archive
  + z-undo, peek delay, auto-advance).

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **428 passed, 0 failed**
  (413 before this feature). New coverage: config shapes for
  `mark_read` (three forms, rejection messages), archive single/batch/
  unknown-folder (end to end over a maildir), pager advance-then-close
  and advance-off, peek delay red/green and close-before-due, flag-op
  cursor advance, tall-panel layout math, mouse coordinate translation,
  picker row geometry, hover style precedence.
- User smoke (R1 #6): passed — driven in a real terminal and approved
  for merge.

## 6. Implementation Report

All six phases landed. Deviations and judgment calls:

- **`MarkRead` stores a `Duration`** (not f64 seconds) so the config
  tree keeps `Eq`; the custom deserializer accepts `"open"`, `"never"`,
  or a positive int/float and rejects everything else at parse time
  with the accepted forms listed.
- **Archive is `move_selected` with a resolved destination** — it
  inherits batch, staging/undo, "already there", and unknown-folder
  behavior for free. `trash_folder` generalized into
  `configured_folder(world, account, pick)`.
- **A latent batch bug died with peek**: the old open path marked read
  via the batch-aware `flag_selected`, so opening a message with marks
  set would have flagged the entire batch as read. Peek's `mark_seen`
  targets exactly the opened message.
- **Auto-advance captures its target before removal** (the next id in
  the visible order), so it is immune to the one-frame staleness of
  the display order; `D`-purge from the pager advances too (same
  path). `:toggle-advance` flips the config resource for the session.
- **Tab hover is click-only**: ratatui-comfy-tabs exposes
  `tab_index_at` for clicks but no per-tab hover state, so the tab bar
  got click-to-switch without a hover highlight. Everything else in
  the approved surface list (index, sidebar, picker hover included)
  is wired; contacts got click + wheel as proposed.
- **Mouse architecture**: plurimus already hit-tests and routes
  per-widget, so each surface owns a `mouse` submodule next to its
  window state (`index/mouse.rs`, `sidebar/mouse.rs`,
  `overlay/mouse.rs`, `explorer/mouse.rs`, `contacts/mouse.rs`, a
  handler in `shell.rs` and `pager/ops.rs`), with shared coordinate
  math and the modal gate in `src/mouse.rs`. Hover rows live in the
  widget window states, survive refresh, and clear when plurimus
  removes `UiHovered`. The picker's click math reuses the renderer's
  geometry helper (`rows_geometry`) so they cannot drift.
- **Handler testing**: the pure math (coordinate translation, picker
  geometry, hover precedence, panel scaling) is unit-tested; the
  interactive dispatch itself needs a real pty and belongs to the
  smoke — synthesizing plurimus's PreUpdate mouse pipeline headlessly
  would test the library, not us.
- `move_cursor` now tolerates a missing `IndexOrder` (the same lenient
  pattern as `batch_ids`), caught by the keymap-layout harness when
  flag ops started advancing.
- **File budget**: `explorer.rs` crossed 300 and was split into
  `explorer/{mod,mouse}.rs`. Pre-existing over-budget files grew
  slightly and remain: `index/mod.rs` (399), `shell.rs` (385),
  `action.rs` (346) — noted as split candidates for a future refactor
  doc, not tackled here to keep this feature reviewable.

Follow-ups: per-tab hover if comfy-tabs grows the API, drag
interactions and compose-editor mouse (out of scope), the file splits
above, and the roadmap's phase-2 advance policies (snooze/mute).

## 7. Testing and Cleanup

Comment pass over the branch diff: comments state invariants (peek's
still-open fire condition, advance-target capture before removal, the
shared picker geometry, hover clearing via `UiHovered`, the border-row
exclusions in click math); none removed. Dead-code check: clippy zero
warnings across the workspace, no unused items. Final verify:
`cargo fmt --all` clean, `cargo clippy --workspace --all-targets`
zero warnings, `CARGO_INCREMENTAL=0 cargo test --workspace`
**428 passed, 0 failed**. Phase 1f — and with it phase 1 — is
complete.
