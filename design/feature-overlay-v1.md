# feature - Overlay - v1

Modal overlay infrastructure: floating panels drawn above the active screen with
a focused input surface — the missing piece the pager's link list wants (per
feature-pager-v1 R1 Q2), and the foundation for future pickers (folders,
contacts), completion popups, and dialogs. Split out so it lands before the
pager builds on it.

## 1. Current Design

- **Input** is a single global plurimus key passthrough owned by the router
  (1a.4): every key resolves synchronously against `Mode` (Normal/CommandLine)
  and the layered keymap tries. CommandLine mode routes keys to
  `cmdline::handle_key`; nothing else intercepts. This design exists for
  burst-safety (mode switches apply mid-burst).
- **plurimus already provides** (verified in source):
  - `WidgetOrder(i32)` — per-widget draw order, a required `Widget` component
    defaulting to 0; higher draws later (on top).
  - A full focus system: `UiFocusable { tab_index, enabled }`, `UiFocused`,
    `UiDisabled`, `UiFocusMessage` (Next/Prev/First/Clear/Set), mouse
    hit-testing that focuses the topmost `WidgetOrder`, and input bindings
    scoped `.focused()` vs `.global()`.
  - The catch: `.focused()` bindings are a _second_ input path that would bypass
    the router's trie — mixing them with the single passthrough reintroduces
    exactly the double-delivery problem 1a.4 eliminated.
- All existing widgets (index, pager-to-be, chrome) spawn with the default order
  0 and fill fixed layout regions; nothing floats.
- `nucleo-matcher` already ranks command completion; `nitidus-ui-kit` is
  bevy-aware (Theme is a Resource) and owns layout helpers.

## 2. Proposal

### 2.1 Input model: router-gated, plurimus-drawn

Overlays follow the CommandLine precedent, not the plurimus focus path: a single
`ActiveOverlay` resource (`Option<PickerState>`) is checked by `route_key`
_before_ Normal-mode trie resolution — when an overlay is open, keys go to
`overlay::handle_key`, which implements the picker's fixed key protocol.
plurimus is used for what it's uniquely good at here — `WidgetOrder` layering
(and later mouse hit-testing) — while keyboard routing stays on the one
passthrough.

plurimus's `UiFocusable`/tab machinery is deliberately _not_ wired in this item:
single-target modals don't need tab order. It becomes relevant for multi-field
surfaces (compose headers), which can adopt it inside their own screen without
touching the router contract. This is recorded as the boundary: **screens and
modals route through the router; intra-widget field focus may use plurimus when
a multi-field widget arrives.**

### 2.2 The v1 overlay: a fuzzy picker

One overlay kind ships now — a centered modal picker:

- `PickerState { title, items: Vec<PickerItem>, filter: String, matches: Vec<u32>, selected: usize, on_select }`;
  `PickerItem { label, detail: Option<String> }`.
- **Typing filters** (nucleo fuzzy ranking, like command completion),
  `<Up>`/`<Down>`/`<C-k>`/`<C-j>` move, `<Enter>` confirms, `<Esc>` cancels —
  fixed protocol, same rationale as the command line: typed characters are
  filter input, so letter keybindings cannot apply.
- `on_select: Box<dyn Fn(&mut World, &PickerItem) + Send + Sync>` — the opener
  decides what selection means; the picker owns no consumer knowledge. Cancel
  just closes.
- Opening API: `overlay::open_picker(world, spec)`; closing restores normal
  routing. One overlay at a time (opening replaces).

### 2.3 Rendering

- A picker widget spawns at startup like the other content widgets but with
  `WidgetOrder(OVERLAY_ORDER)` (e.g. 100) and renders only while an overlay is
  open — above whatever screen is active.
- `nitidus-ui-kit::layout` gains `centered_panel(area, width_pct, max_height)`
  for the floating rect; the panel renders a themed bordered block (title from
  the spec), the filter line, and the ranked list with the selected row
  highlighted (same state ladder as the index).
- List rows are virtual-friendly (render only the visible window) but v1 pickers
  are small; no pagination machinery beyond scroll-to-keep- selected-visible.
- Statusline center shows nothing new; the overlay is self-describing.

### 2.4 Wiring and demo consumer

- `overlay.rs` in the bin crate: plugin (resource + widget spawn + refresh),
  `handle_key`, `open_picker`.
- To prove the machinery independently of the pager, `:sort` with no arguments
  changes from "reset to date" to **opening a sort-key picker**
  (date/from/subject/unread/flagged, `-r` variants) — a real consumer with
  trivial semantics. (`:sort <key>` keeps working unchanged; the bare-`:sort`
  reset moves to `:sort date`.)
- The pager item then consumes `open_picker` for its link list (and can use it
  for parts if `]`/`[` ever feels insufficient).

## 3. Discussion

### 3.1 R1 Questions

1. **Input boundary** (§2.1): router-gated overlay handler; plurimus supplies
   draw order now and (mouse/tabbing) primitives later; `.focused()` bindings
   stay out of the app until a multi-field widget needs intra-widget focus.
   Confirm this boundary?
2. **Fixed picker keys** (§2.2): typing filters, so navigation is
   `<Up>`/`<Down>` (+ `<C-j>`/`<C-k>`), `<Enter>`, `<Esc>` — hardcoded like the
   command line rather than a `picker` keymap context. OK, or do you want it
   rebindable from day one?
3. **Selection semantics** (§2.2): world-closure `on_select` (opener owns the
   meaning) vs dispatching a command string. Closure proposed.
4. **Demo consumer** (§2.4): bare `:sort` opens the sort picker (behavior change
   from "reset to date"). Good demo, or keep `:sort` as-is and land the overlay
   consumer-less until the pager?
5. **Visuals** (§2.3): centered, ~50% width, height fit-to-items capped ~60%,
   themed border + title, no backdrop dimming (tachyonfx can add it later). OK?
6. **Placement** (§2.3): `centered_panel` layout math in ui-kit,
   picker/plugin/handler in the bin crate. Confirm?

### 3.2 R1 Answers

1. I think we will need the focused bindings sooner than later. Take a look how
   overlays are done in vcard_tui for an example, and modify as best fits this
   application if vcard_tui doesn't fit our current codebase.
2. rebindable
3. whichever you prefer
4. keep as is
5. ok
6. confirm

### 3.3 R2 Design Notes (vcard_tui survey + reconciliation)

**What vcard_tui does** (`ui/export.rs`, `common/builders/popup.rs`): a
popup is a *set of entities* keyed by a marker enum, spawned/despawned
by an `on_change` system watching a state resource. Each entity gets a
builder (widget + marker), a layout slot at a higher z (10/11 over base
screens), a focus index (`UiFocusable`), and **plurimus
`UiInputBinding`s** — `.focused()` Enter on fields/buttons, `.global()`
Esc on the popup frame, mouse bindings on buttons. The panel renders
ratatui `Clear` then a bordered block. There is no central router;
plurimus delivers input per entity.

**What transfers directly**: resource-driven spawn/despawn, the
`Clear`-backed floating panel, layered draw order (`WidgetOrder` here),
and the plurimus focus *components* — the picker entity is
`UiFocusable`, receives `UiFocusMessage::Set` on open and `Clear` on
close, so `UiFocusState` is real and plurimus mouse hit-testing works.
`PlurimusUiPlugin` (already in our app) registers all of it.

**What must be modified**: keyboard delivery. R1-2 wants picker keys
*rebindable*, and plurimus bindings are code-registered — the exact
reason 1a.4 built the keymap router. Per-entity `.focused()` *keyboard*
bindings would also be a second delivery path beside the global
passthrough (the 1a.4 double-delivery problem). So keyboard stays on
the router, but becomes **focus-aware**: with an overlay open, keys
resolve against a new rebindable `picker` keymap context
(single-key bindings only — no chord waits, because any unbound
printable character is filter input), and everything unbound-printable
types into the filter. Global bindings do not leak through (typing `q`
must filter, not quit) — the command-line precedent. `.focused()`
plurimus bindings do enter the app here for the **mouse** path (the
router owns only keys), and per-field keyboard focus remains available
to future multi-field surfaces via the same components.

Commands stay the vocabulary: `:confirm`/`:cancel` are new;
`:next`/`:prev` are *reused* — `Action::Cursor` dispatches to the
picker selection while an overlay is open, so `[picker]` in keys.toml
rebinds navigation like any other context.

## 4. Plan

**Phase 1 — ui-kit layout**: `centered_panel(area, width_pct,
max_height)` returning the floating rect (clamped to the area) +
tests.

**Phase 2 — overlay module** (bin `overlay.rs`):

1. `ActiveOverlay(Option<PickerState>)`;
   `PickerState { title, items, filter, matches, selected, on_select:
   Box<dyn Fn(&mut World, usize) + Send + Sync> }` (closure — R1-3
   delegated). `open_picker(world, PickerSpec)`; nucleo re-rank on
   filter edits; selection clamp.
2. vcard_tui-style `on_change` system spawns/despawns the picker entity
   (`Widget` render fn, `WidgetLayout` from `centered_panel`,
   `WidgetOrder(100)`, `UiFocusable`), sending
   `UiFocusMessage::Set`/`Clear` on open/close. Render: `Clear`,
   themed bordered block + title, filter line, ranked list with
   selected-row highlight, scroll-to-keep-visible.
3. Router: `route_key` checks `ActiveOverlay` before Normal resolution
   → `overlay::handle_key` (single-key `picker`-context lookup; exact →
   `apply_action`; unbound printable → filter; Backspace edits).
4. Actions/commands: `Action::OverlayConfirm`/`OverlayCancel`
   (`:confirm`/`:cancel`); `Action::Cursor` dispatch gains the
   overlay branch; `picker` added to `KNOWN_CONTEXTS` with defaults
   `<Down>`/`<C-j>`/`<Up>`/`<C-k>`/`<Enter>`/`<Esc>`.

**Phase 3 — tests**: picker filter/selection math; integration through
`route_key` (typing filters, navigation, confirm runs the closure and
closes, cancel closes, keys route normally after close, global `q`
does not leak while open); `[picker]` rebinding override; keymap
context compiles.

**Phase 4 — verification**: clippy, full workspace tests with counts.
No consumer ships this item (R1-4), so visual pty verification lands
with the pager's link picker; the widget path is covered to the render
fn boundary by the integration tests.

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **151 passed, 0
  failed** (was 144): nitidus unit 88 + index 5 + overlay 6,
  nitidus-contacts 1, nitidus-mail 14+7+8+6, nitidus-ui-kit 16.
- The six overlay integration tests drive real key events through
  `route_key`: fuzzy filtering selects by original index, `<Down>` /
  `<C-j>` / `<Up>` navigation, `<Esc>` cancel restores normal routing
  (a following `q` quits), modality (`q` filters instead of quitting,
  `:` filters instead of opening the command line), `[picker]`
  rebinding (`<C-n>` → `:next`), and widget spawn/despawn tracking the
  resource.
- Visual pty verification deferred to the first consumer (the pager's
  link picker, next item), per R1-4 — no command opens a picker yet.

## 6. Implementation Report

Implemented per the §3.3 reconciliation:

- `overlay/mod.rs` — `ActiveOverlay`/`PickerState` (nucleo re-rank on
  filter edits, selection clamp), `open_picker`/`close`/`confirm`
  (closure runs *after* the overlay closes, so it may open another),
  the vcard_tui-style `sync_picker_entity` spawn/despawn system
  (`WidgetOrder(100)`, `UiFocusable`, `UiFocusMessage::Set`/`Clear`),
  and `handle_key`. `overlay/render.rs` — `Clear`-backed bordered
  panel, filter line, centered-scroll list.
- Router: one added check, mirroring the CommandLine gate. The picker
  context resolves single keys only; `Action::Cursor` grew the overlay
  branch so `:next`/`:prev` (and user rebinds) drive the selection.
- `OverlayPlugin` registers `UiFocusMessage` itself (idempotent with
  `PlurimusUiPlugin`) so headless test apps work without the full
  plurimus stack — the only wrinkle found during implementation.
- `centered_panel(_layout)` landed in ui-kit; `index::apply_motion` is
  re-exported and reused for picker row math.
- New commands `:confirm`/`:cancel`; `picker` joined `KNOWN_CONTEXTS`
  with `<Down>`/`<Up>`/`<C-j>`/`<C-k>`/`<Enter>`/`<Esc>` defaults.

Follow-ups:

- First consumer + visual check: the pager's link picker (next item).
- Mouse: plurimus `.focused()`/mouse bindings on the picker entity
  (click-to-select) — the mouse path doesn't conflict with the router;
  add when mouse support gets a pass generally.
- Multi-field dialogs (compose) adopt `UiFocusable` tab order within
  their screen when they arrive; the §3.3 boundary stands.

## 7. Testing and Cleanup
