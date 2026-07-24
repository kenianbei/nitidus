# feature - Index View - v1

Roadmap item 1b.8. The virtualized message index: a windowed table over
`MailStore` (100k-row target) with selection, scrolling, a sorting suite, flag
display, and the first flag operations. This is the item that puts real mail on
screen.

## 1. Current Design

- `MailStore` (1b.7) holds date-desc sorted `Vec<EnvelopeSummary>` per
  `(AccountId, FolderId)` plus folder lists; `SyncTracker` +
  `bootstrap::request_sync` exist for lazy first-view syncs (INBOX is already
  eager). `EnvelopeSummary` is
  `{ id, subject, from_display, from_addr, date_epoch_secs, flags }`; `Flags`
  covers SEEN/ANSWERED/FLAGGED/DELETED/DRAFT.
- The shell's `ContentPane` is an empty themed `Block` occupying
  `layout::content_layout()`; nothing renders mail.
- The router resolves every key against **only** `CONTEXT_GLOBAL`;
  `KNOWN_CONTEXTS` already reserves `"index"` but nothing consults it.
  `KeymapMatch` is Exact/Prefix/Unbound; exact fires immediately.
- `Action` has five variants (Quit, OpenCommandLine, TabNext, TabPrev, Echo);
  `apply_action` mutates the world directly. Commands are the single vocabulary
  (keys and `:` line share `parse_command`).
- `MailCommand::SetFlags { folder, id, flags }` exists end-to-end (maildir
  renames with the `:2,` protocol); the rename fires the watcher, so a flag
  write produces `FolderChanged` → re-sync → store reconcile for free.
- Widgets follow the plurimus pattern: spawn once, refresh systems gated on
  `is_changed()`, `Widget::from_render_fn_with_state`.

## 2. Proposal

New `crates/nitidus/src/index/` module (`mod.rs` plugin + systems, `view.rs`
selection/order/sort, `render.rs` row building), replacing the empty
`ContentPane` as the mail tab's content.

### 2.1 View state and virtualization

- `IndexView` resource: `account`, `folder` (fixed to first configured account +
  INBOX this item), `selected: Option<EnvelopeId>` with a cached position, `top`
  (first visible row), `sort: SortMode`.
- **Selection is identity-based**: the cursor follows the envelope id across
  re-syncs and re-sorts; if the id disappears (message deleted externally), the
  cursor clamps to the same position in the new order.
- `IndexOrder`: the display permutation (`Vec<u32>` into the store slice). For
  the default `date` sort it is the identity (the store is already date-desc —
  zero cost); other sorts recompute on store or sort-mode change (O(n log n),
  only paid when a non-default sort is active).
- The refresh system rebuilds the widget state **only for the visible window**
  (`top .. top + viewport height`), never all rows. Viewport height is captured
  by the render fn into its state each frame and read back by the refresh/clamp
  logic (one frame of lag after a resize, self-correcting).

### 2.2 Rendering

- Columns: flags (4) · date (12) · from (fixed share, truncated) · subject
  (rest). Flag cell chars: `N` unseen, `F` flagged, `R` answered, `D` deleted,
  `d` draft.
- Row styling from the theme state ladder: unseen rows bold, the selected row
  uses `selected`, deleted rows dimmed. Selected+unseen compose.
- Smart short dates: `HH:MM` for today, `Jul 24` for this year, `2024-02-15`
  otherwise (computed against local time at render).
- Empty states: "no accounts configured" / "empty folder" centered in the pane.
- Statusline left segment gains position: `mail ⋅ 1/1 ⋅ 3/128` (selected index /
  total in folder).

### 2.3 Actions, commands, bindings

New commands (all context-free in the parser, meaningful in the index):

| command                   | action             | default index binding    |
| ------------------------- | ------------------ | ------------------------ |
| `:next` / `:prev`         | cursor down/up     | `j`/`k`, `<Down>`/`<Up>` |
| `:next-page`/`:prev-page` | page down/up       | `<PageDown>`/`<PageUp>`  |
| `:first` / `:last`        | jump to top/bottom | `gg` / `G`               |
| `:sort <key> [-r]`        | set sort mode      | —                        |
| `:read` / `:unread`       | set/clear SEEN     | —                        |
| `:flag` / `:unflag`       | set/clear FLAGGED  | —                        |
| `:toggle-read`            | toggle SEEN        | `N`                      |
| `:toggle-flag`            | toggle FLAGGED     | `F`                      |

- Sort keys: `date`, `from`, `subject`, `unread`, `flagged` (fields that exist
  on `EnvelopeSummary`; `size` waits for a summary field). `-r` reverses.
  `:sort` with no args resets to `date`.
- Flag ops act on the selected envelope: **optimistic** store update +
  `MailCommand::SetFlags`; the maildir rename's watcher event then re-syncs and
  confirms (or corrects) — failures surface via the existing `JobFailed`
  statusline path.

### 2.4 Context-aware routing

`resolve_now` gains context layering: in Normal mode the active context is
`"index"` (derived trivially for now; tabs/screens will drive it later).
Resolution: exact match in the context wins; otherwise if either context or
global reports a prefix, wait; otherwise the global exact fires; else unbound.
Context bindings therefore shadow global ones, and multi-key sequences work in
both layers.

### 2.5 First-view sync

Opening the index for a folder calls `bootstrap::request_sync` unless
`SyncTracker` already tracks it — a no-op for INBOX this item (eager at
registration) but wired so folder switching (sidebar item) inherits the lazy
contract without new plumbing.

## 3. Discussion

### 3.1 R1 Questions

1. **Sort suite scope.** `date | from | subject | unread | flagged` with `-r`,
   default `date` (descending, store-native). `size`/`thread` deferred to the
   items that add the data. OK?
2. **Flag command set.** Explicit `:read`/`:unread`/`:flag`/`:unflag`
   (scriptable) plus `:toggle-read`/`:toggle-flag` for the default bindings
   `N`/`F` (neomutt muscle memory). Six commands total — or would you rather
   trim to just the two toggles for now?
3. **Cursor command names.** aerc-style `:next`/`:prev`/`:next-page`/
   `:prev-page`/`:first`/`:last` as proposed? (`gg`/`G` + `j`/`k` + arrows +
   PageUp/PageDown as default bindings.)
4. **Context shadowing semantics** (§2.4): context exact > any prefix > global
   exact. The subtle case: `g` bound globally, `gg` in index — pressing `g`
   waits for the chord rather than firing the global. This matches aerc/vim
   expectations; confirm?
5. **Optimistic flag ops** (§2.3): store updates immediately, engine write
   follows, watcher re-sync reconciles. The alternative is pessimistic (wait for
   re-sync; laggy UI). Confirm optimistic?
6. **Columns/date format** (§2.2): fixed flags/date widths, from capped, subject
   fills, smart short dates. Any preference changes (e.g. ISO dates everywhere,
   different flag chars)?

### 3.2 R1 Answers

1. ok
2. six is good
3. yes
4. confirm
5. optimistic
6. nope, it's good

## 4. Plan

**Phase 1 — layered key routing** (compiles, tests green):

1. `keymap.rs`: `resolve_layered(context, keys)` implementing context
   exact > any prefix > global exact > unbound; unit tests for
   shadowing, prefix-wait (`g` global vs `gg` index), and fallback.
2. `router.rs`: Normal-mode resolution goes through the layered lookup
   with the active context (constant `"index"` for now).

**Phase 2 — actions and commands**:

1. `action.rs`: `Action::Cursor(Motion)`, `Action::Sort(SortMode)`,
   `Action::Flag { flag, op }` (set/clear/toggle) + the thirteen command
   registrations from §2.3; `apply_action` delegates to `index::`
   world functions. Sort-argument parsing lives with `SortMode`.
2. `keymap.rs`: `DEFAULT_INDEX_BINDINGS` (`j`/`k`/arrows, PageUp/Down,
   `gg`/`G`, `N`, `F`).

**Phase 3 — the index module**:

1. `store.rs`: `set_flags` mutator (optimistic path needs a targeted
   in-memory write).
2. `index/view.rs`: `IndexView`, `SortMode` + order computation,
   identity-based selection with clamp, motion application, flag ops
   (store mutation + `MailCommand::SetFlags`).
3. `index/render.rs`: visible-window row building, flag chars, column
   truncation, smart short dates (local time via `jiff`).
4. `index/mod.rs`: `IndexPlugin` — widget spawn over the content region,
   change-gated refresh, viewport-height feedback from the render fn,
   `IndexStatus` resource for the statusline, first-view
   `request_sync` hook.
5. `shell.rs`: left segment appends `selected/total` from `IndexStatus`.
6. Tests: sort orders, selection stability across reorder/prune, page
   motions, optimistic flag flow (store + real-maildir rename), widget
   state contains rendered subjects, statusline position.

**Phase 4 — verification**: `cargo clippy --workspace`,
`CARGO_INCREMENTAL=0 cargo test --workspace` with pass counts, pty smoke
test against a real maildir (rows visible, cursor moves, `F` renames the
file with the `:2,` flag, warm start still renders).

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **128 passed, 0 failed**
  (was 109): nitidus unit 83 + index integration 4, nitidus-contacts 1,
  nitidus-mail 7+5+7+6, nitidus-ui-kit 15.
- pty smoke test (80×24, isolated XDG dirs, maildir with three messages
  across `new/` and `cur/`, one pre-seen, dates spanning today/this
  year/2024), replayed through a terminal emulator (`pyte`) to recover
  the final screen:
  - Cold run: three rows rendered date-descending with `N` flags cells,
    smart dates (`Jul 21`, `Jul 20`, `2024-02-15`), from and subject
    columns; statusline `mail ⋅ 1/1 ⋅ 2/3` after a `j` press moved the
    cursor. Pressing `F` renamed the selected message on disk to
    `:2,FS` and the re-synced cache row carries SEEN|FLAGGED.
  - Warm run: identical screen straight from the cache, flag state
    persisted, clean exit.
  - Lesson recorded: debug-build startup takes ≳2s before the first
    frame; smoke-test keypresses need a ~4s lead.

## 6. Implementation Report

Implemented as planned. Specifics worth noting:

- **Layered routing** (`Keymaps::resolve_layered`): context exact >
  any-layer prefix > global exact > unbound, exactly Q4's semantics —
  the `g`-waits-for-`gg` case is a regression test. The router passes a
  constant `CONTEXT_INDEX` for now.
- **Actions** grew three compact variants (`Cursor(Motion)`,
  `Sort(SortMode)`, `Flag { flag, op }`) rather than thirteen flat ones;
  the command registry maps all thirteen names onto them.
- **`index/` module** landed as four files (`mod.rs` plugin + refresh
  systems, `view.rs` pure sort/selection/motion logic, `render.rs` row
  formatting, `ops.rs` world-mutating operations). Selection anchoring
  and scroll clamping write through `bypass_change_detection` — they are
  derived state, and tracked writes would re-trigger the refresh every
  frame.
- **Virtualization**: the window is `top .. top + max(viewport, 100)`
  rows; the render fn records the real height into its widget state each
  draw, and refresh/motion logic reads it back (documented one-frame lag
  on resize). The default date sort is the identity permutation over the
  store's native order.
- **Optimistic flags** confirmed end-to-end in an integration test: the
  store shows FLAGGED before the engine write lands, then the maildir
  file rename (`:2,F`) is observed on disk.
- Dates use `jiff` (new dependency) for local-timezone smart formatting.
- Statusline position (`⋅ 2/3`) flows through a small `IndexStatus`
  resource so `shell.rs` needs no knowledge of index internals.

Follow-ups for later items:

- Folder switching / account switching UI (sidebar item) — the
  first-view `request_sync` hook and lazy `SyncTracker` contract are
  already wired for it.
- `FolderMeta` unread counts are not updated optimistically by flag ops;
  they refresh on the next `ListFolders`. Revisit with the sidebar.
- Column widths are character-count based (no Unicode display-width
  handling); revisit if CJK/emoji subjects misalign.
- The selected-row highlight and unseen-bold styling exist but were not
  visually verified in the pty (pyte replay drops styling); worth a look
  in a real terminal session.

## 7. Testing and Cleanup
