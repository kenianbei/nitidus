# feature - Help Overlay - v1

A `?` hotkey (and `:help` command) showing the key bindings that work right now
— sequence, command, and a one-line description — in a searchable overlay. This
pulls the "live key table" half of roadmap item 27 (comfort features) forward;
the rest of item 27 stays put.

## 1. Current Design

Everything needed exists, but nothing surfaces it to the user:

- `Keymaps` compiles defaults + `keys.toml` overrides into per-context tries
  (`TrieNode { action, children }`). The trie stores the parsed `Action` but
  **not the command string** it came from, and the contexts map is private —
  there is no way to enumerate bindings today.
- Resolution is layered (context exact > any-layer prefix > global exact), so
  "what works right now" is the active context's bindings plus non-shadowed
  globals. The router derives the active context from sidebar focus and the
  `Screen`.
- `command.rs` holds the `CommandSpec` table (name, aliases, parse fn) — no
  human descriptions.
- The picker overlay (1b.10's overlay feature) is a searchable list with
  `label` + optional `detail` per item, nucleo fuzzy filtering on unbound
  printables, rebindable `[picker]` context keys, and an `on_select` closure —
  the natural surface for a filterable key table.
- Key sequences render through crokey's `KeyCombinationFormat`
  (`router::format_keys`).

## 2. Proposal

### 2.1 Enumerable bindings

`TrieNode` gains the source command string (stored at bind time, removed on
unbind), and `Keymaps` gains
`bindings(context) -> Vec<BindingRow { keys: String, command: String }>` walking
the context trie depth-first with crokey-formatted sequences. The help view asks
for the active context's rows plus the global rows whose sequences the context
does not shadow.

### 2.2 Command descriptions

`CommandSpec` gains `summary: &'static str` (one line, e.g. `fold-all` →
"collapse every thread"), and `command.rs` exposes
`describe(name) -> Option<&'static str>`. The help rows join on the command name
after stripping arguments, so `:command-line folder-create` still describes as
the command-line entry.

### 2.3 The overlay

`?` (bound in index, pager, and sidebar contexts) and `:help` open the picker
titled `keys — {context}`:

- Label: `za  fold` (formatted sequence, padded, then command name).
- Detail: the summary, with `(global)` appended for non-shadowed global rows.
- Rows are sorted context-first then by sequence; the nucleo filter searches
  labels as usual.
- **Enter executes the selected binding** — the on_select closure closes the
  picker and applies the row's `Action`, making help double as a command
  palette. Esc just closes.

### 2.4 Scope guard

No new widgets, screens, or layouts; ~1 new keymap walk + a table-column
change + one picker call. `:help` with no arguments only (topic pages are a
later item).

## 3. Discussion

### 3.1 R1 Questions

1. **Picker as the surface.** Reuse the overlay picker (searchable, centered
   panel) rather than a dedicated screen or side pane — confirm?
2. **Scope: what works now.** Show the active context + non-shadowed globals
   only (with `?` pressed in the pager you see pager + global rows), not all
   contexts at once. The context is named in the title. OK, or would you rather
   see every context grouped in one list?
3. **Enter runs the binding.** Selecting a row executes its action
   (command-palette behavior). Confirm, or should Enter just close?
4. **Descriptions in the command table.** Adding a one-line `summary` to every
   `CommandSpec` (~45 entries) is the bulk of the diff and also benefits the
   future `:help` topics and command-line completion. OK?
5. **`?` placement.** Context bindings (index/pager/sidebar), not global — the
   picker keeps `?` for filtering and the command line for typing. `:help`
   covers everywhere else. Confirm?

### 3.2 R1 Answers

1. confirm
2. show active context, but with ability to toggle what is shown in the overlay
   with another key, and show grouped hotkeys.
3. confirm
4. Yes, and we should include descriptions with :commands if possible, not sure
   where though as the command is on the bottom bar. Any ideas? Also, I really
   like the command palette in the Helix editor, where all commands starting
   with current text are shown above in a panel, and tab cycles on the possible
   commands.
5. confirm

### 3.3 R2 Notes

1. **Scope toggle (R1-2).** `<Tab>` inside the help picker toggles between
   `keys — {context}` (active + non-shadowed globals) and `keys — all`
   (every context). Grouping: rows sort by context (global, index, pager,
   sidebar, picker, command_line) then sequence, and in all-scope each
   label carries a `[context]` tag — the picker stays a flat filterable
   list, so no non-selectable header machinery is needed. The toggle is a
   normal picker-context binding (`:help-scope`) that no-ops when the open
   picker is not the help overlay.
2. **Helix-style completion panel (R1-4).** This is where command
   descriptions live: a bottom-anchored panel that appears above the
   statusline while the command line is open and matches exist — one row
   per candidate, `name — summary`, current selection highlighted,
   height capped (8 rows). `<Tab>` keeps its existing cycle behavior, now
   with the list visible; typing refines the match set live (the existing
   nucleo matcher, which ranks prefix matches first). The panel follows
   the overlay pattern (spawned entity, `WidgetOrder` above the content,
   draws nothing when inactive) so it paints correctly over the index.

## 4. Plan

Each phase leaves the workspace compiling, clippy-clean, and tests green.

**Phase 1 — enumerable bindings + summaries.** `TrieNode` stores the
source command string (set on bind, cleared on unbind); `Keymaps` gains
`bindings(context) -> Vec<BindingRow { keys, command }>` (depth-first,
crokey-formatted) plus a shadow-aware
`help_rows(context) -> Vec<HelpRow>` merging the context and global
layers. `CommandSpec` gains `summary: &'static str` for all entries;
`command::describe(input)` strips arguments and joins on name/alias.
Unit tests for the walk, shadowing, and describe.

**Phase 2 — help overlay.** `src/help.rs`: `open(world, HelpScope)`
builds picker items (labels aligned `sequence  command`, `[context]`
tag in all-scope, summary as detail; Enter parses and applies the row's
command), `toggle_scope(world)` reopens with the other scope when the
help picker is open. New commands `help` + `help-scope`;
`Action::{Help, HelpScope}`; `?` bound in index, pager, and sidebar
contexts; `<Tab>` bound in picker context to `:help-scope`. A
`HelpState` resource tracks the open scope so the toggle no-ops for
other pickers. Integration tests: `?` opens with context rows, Tab
widens to all contexts, Enter executes a binding.

**Phase 3 — command-line completion panel.** Split `cmdline.rs` into
`cmdline/mod.rs` + `cmdline/panel.rs`. The panel entity spawns with
`WidgetOrder` above content, anchored above the statusline (max 8
rows); a refresh system rebuilds rows from the live buffer via
`complete_command`, joining summaries, highlighting the Tab-cycle
selection; it draws nothing outside CommandLine mode. Integration
tests: panel rows follow the buffer, Tab cycling moves the highlight.

**Phase 4 — smoke + docs.** Pty smoke: `?` in the index over the live
corpus, Tab scope toggle, Enter executing a row; `:` typing showing the
panel with descriptions, Tab cycling visibly. §5/§6 recorded.

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **228 passed, 0 failed**
  (was 218 pre-feature: +3 keymap enumeration tests, +1 describe test,
  +4 help-overlay integration tests, +1 completion-view test, +1 panel
  windowing test).
- Integration coverage: `?` opens the index context with summaries and
  `(global)` tags, shadowed globals absent; Tab round-trips
  current ↔ all with `[context]` grouping; Enter executes the filtered
  selection; non-help pickers ignore the scope toggle.
- Pty smoke over the live corpus: `?` showed `keys — index` with
  aligned rows and summaries; Tab flipped to `keys — all` with
  `[global]`/`[index]` groups; `:fo` raised the completion panel above
  the statusline (7 candidates with summaries), Tab cycled to `:fold`,
  Esc dismissed with the index cells fully restored.

## 6. Implementation Report

Implemented per plan, with these findings:

- **Key display formatting:** crokey renders `M` as `Shift-m` by
  default; `format_keys` now uses implicit shift so help (and the
  chord hint) match how bindings are written in keys.toml.
- **Two persistent-buffer lessons re-learned** in the panel: a
  `Paragraph` sets style but does not overwrite cells, so the panel
  renders `Clear` first; and disabling a widget leaves its pixels
  behind, so the panel spawns/despawns like the picker overlay
  (despawn is what lets the widgets underneath repaint).
- The help/scope round-trip needs no state resource: the picker title
  is the discriminator (`ActiveOverlay::title()` accessor added), so a
  stale flag can never mis-fire on other pickers.
- **300-line splits:** the summary column pushed three files over the
  limit, so `keymap` became `mod/defaults/rows`, `command` became
  `mod/table`, and `cmdline` gained `history.rs` beside `panel.rs`.
  Behavior unchanged; covered by the existing tests.
- Follow-ups: the panel shows nothing while arguments are typed (a
  per-argument hint line would need command signatures — later);
  mouse clicks on help rows ride the roadmap-27 mouse pass.

## 7. Testing and Cleanup
