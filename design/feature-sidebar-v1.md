# feature - Folder Sidebar - v1

Roadmap item 1b.13. A folder sidebar beside the index: per-account folder trees
with unread counts, collapse/expand, and folder switching wired into the
existing lazy-sync/cancellation contract. Selecting a folder in another account
also switches accounts — the first account-switching UI. Folder
create/delete/rename rounds out the item.

## 1. Current Design

Folders exist end-to-end but have no UI:

- `MailBackend::list_folders` → `MailCommand::ListFolders` →
  `MailEvent::Folders` → `MailStore::set_folders`; issued once per account at
  registration. Warm start hydrates folders from the envelope cache.
- `FolderMeta { id, name, unread, total }`; maildir discovery decodes Maildir++
  dot-dirs (`.Archive.2024` → display `Archive/2024`) and counts `new/`
  (unread) + `cur/` at scan time. The kenianbei corpus now has 11 label folders
  including nested-looking `[Gmail]/…` names.
- Counts go stale: nothing re-issues `ListFolders` after registration, and
  optimistic flag edits touch envelopes, not `FolderMeta` (a known deferred item
  from 1b.8).
- `IndexView { account, folder, .. }` is fixed to the first configured account's
  INBOX. `first_view_sync` already implements the lazy contract: any newly
  viewed folder gets `request_sync` (cancel-supersede) on first sight — folder
  switching inherits this for free.
- The content region is a single full-width rect (`layout::content_layout`); the
  index and pager widgets both claim it, gated by the `Screen` resource
  (inactive screens render nothing). `WidgetLayout` holds an `Arc` layout fn
  computing a rect from the frame area — it cannot read ECS state, but the
  component can be replaced to re-layout.
- Router context comes from `Screen` (index/pager) plus the CommandLine and
  overlay gates; contexts are rebindable via `keys.toml`.
- The backend trait has **no folder create/delete/rename**; neither do
  `MailCommand`/`MailEvent`. The cache persists folders per account (replaced
  wholesale on each `Folders` event).

## 2. Proposal

### 2.1 Layout and visibility

A `Sidebar` widget in a fixed-width left column (`SIDEBAR_WIDTH = 24`) of the
content region, visible by default and toggleable. `nitidus-ui-kit` gains
`sidebar_layout()` and `main_layout()` (content minus sidebar). A
`SidebarState { visible, focused, selected, top, collapsed }` resource drives
everything; a system swaps the `WidgetLayout` components on the
sidebar/index/pager widgets when `visible` flips, so hiding the sidebar gives
the index the full width again. The sidebar stays visible on both Index and
Pager screens (it never overlaps; the pager just gets the main column).

### 2.2 Tree model

A pure `sidebar/tree.rs` builds display rows from `MailStore::folders` across
all configured accounts, in config order:

- One section per account (account name as a non-selectable header line when
  more than one account is configured).
- Folder display names split on `/` into a tree (`[Gmail]/Sent Mail` nests under
  `[Gmail]`); intermediate nodes without a real folder are synthetic
  (non-selectable, aggregate unread). INBOX sorts first, then lexicographic.
- Rows carry `depth`, `has_children`, `is_collapsed`; collapse state keyed by
  `(account, path)` in `SidebarState`, INBOX-siblings visible by default,
  `[Gmail]` collapsed by default.
- Unread badge per row: `name (unread)` when unread > 0, dimmed count; selection
  highlight reuses the index row style.

### 2.3 Unread counts

Two-source counts, freshest wins:

- Folders the store has envelopes for (synced this session): unread derived live
  from `MailStore` flags — optimistic flag edits update the badge the same
  frame.
- Unsynced folders: the `FolderMeta.unread` snapshot from discovery/warm-start.
- `MailEvent::FolderChanged` already triggers a folder re-sync; the same handler
  now also re-issues `ListFolders` for that account, refreshing snapshots for
  folders the watcher saw change. This closes the deferred 1b.8 staleness item
  well enough for tier-1 use.

### 2.4 Folder switching

Selecting a folder (Enter) sets `IndexView.{account, folder}`, clears
selection/scroll/fold state, and returns focus to the index. `first_view_sync`
lazily scans folders on first view (existing contract). Wiring to cancellation:
before switching, any in-flight scan of the _outgoing_ folder is cancelled via
the tracker's `in_flight_job` + `MailCommand::Cancel` — leaving a folder
abandons its scan; returning re-requests it (`request_sync` supersedes cleanly).
The pager closes if open (switching folders invalidates the open message's
context). `ThreadSet`/`IndexOrder` already re-key on account/folder +
generation, so threading and sorting follow automatically.

### 2.5 Folder create/delete/rename

Backend + protocol growth, maildir implementation:

- `MailBackend` gains `create_folder(&FolderId)`, `delete_folder(&FolderId)`,
  `rename_folder(&FolderId, &FolderId)`; `MailCommand` mirrors them; completion
  re-uses `MailEvent::Folders` (each op returns the refreshed list) with
  `JobFailed` carrying errors.
- Maildir impl: create = mkdir `cur/new/tmp` under the Maildir++ dot-name
  encoded from the display path; rename = directory rename (children of a
  renamed parent are renamed too); delete = remove the directory **only if it
  holds no messages** (`cur`/`new` empty) and has no child folders — destructive
  deletion is refused with an error, not confirmed with a prompt. INBOX cannot
  be renamed or deleted.
- UI: `:folder-create <path>`, `:folder-rename <new-path>` (acts on the
  sidebar-selected folder), `:folder-delete` (selected folder, empty-only)
  through the existing command line, plus sidebar keys bound to the same actions
  pre-filling the command line where a name is needed.
- Store/cache: the refreshed `Folders` event replaces both (existing
  wholesale-replace semantics); a deleted folder that is currently viewed
  switches the view back to INBOX.

### 2.6 Input and keybindings

New rebindable `[sidebar]` router context, active while `SidebarState.focused`
(checked after the CommandLine and overlay gates, before the Screen-derived
context). Defaults:

- Global/index: `b` toggle sidebar visibility, `Tab` focus sidebar.
- Sidebar: `j`/`k`/arrows move, `gg`/`G` first/last, `Enter` open folder (focus
  returns to index), `za` toggle collapse, `zM`/`zR` collapse/expand all,
  `Tab`/`Esc` return focus without switching, `c`/`r`/`D` pre-fill the
  create/rename/delete commands.
- Focus is router-level only (same §3.3 boundary as the picker — plurimus
  `.focused()` keyboard bindings stay unused).

## 3. Discussion

### 3.1 R1 Questions

1. **Sidebar scope on the pager screen.** Proposed: sidebar stays visible beside
   the pager (it owns its column; the pager takes the main column). Mutt hides
   it by default in the pager; aerc keeps it. Keep it visible, or hide it while
   reading?
2. **Account switching via sidebar.** Selecting a folder under the other
   account's section switches `IndexView.account` too — first account-switching
   UI, ahead of the 1d account work. Confirm this is in scope (the alternative
   is sidebar shows only the active account until 1d)?
3. **Delete semantics.** Refuse to delete non-empty folders (error in the
   statusline) instead of building a confirm prompt now. Acceptable, or do you
   want a y/n confirm flow in this item?
4. **Synthetic parents.** `[Gmail]/Sent Mail` under a collapsed synthetic
   `[Gmail]` node, aggregate unread on the parent while collapsed — matches your
   corpus. Any preference for flat display (full paths, no tree) as a config
   option now, or defer?
5. **Default visibility.** Sidebar shown by default at width 24, `b` to toggle,
   no config key yet (config lands when there's a config item worth batching).
   OK?
6. **`FolderChanged` → `ListFolders` refresh.** This closes the stale-count
   deferred item for watched maildirs. Fine to fold into this item?

### 3.2 R1 Answers

1. keep visible.
2. yes, in scope
3. refuse to delete
4. defer, we will need a settings UI at some point though
5. ok
6. yes

Does it make sense at all to pull in a tui library for trees
(tui-rs-tree-widget)?

### 3.3 R2 Notes

1. **tui-tree-widget: not worth it here.** The sidebar's actual complexity
   (two-source unread counts, account sections, synthetic nodes, selection
   and collapse keyed to `(account, path)`) is outside what the crate
   models; its `TreeState` (open-set/selection/scroll) would have to be
   bridged into our resources anyway since the router owns all keyboard
   input. The pure flatten-visible-rows step it would save is ~50 lines in
   the same shape as the index's thread fold (`OrderEntry` with
   depth/collapse), which we already own and test. Skipping also avoids a
   third-party ratatui-version compat axis. Same reasoning as the
   tui-widget-list decision on the index.
2. A **settings UI** is noted as a future roadmap concern (R1-4); until
   then display preferences accumulate in `config.toml`.

## 4. Plan

Each phase leaves the workspace compiling, clippy-clean, and tests green.

**Phase 1 — folder CRUD in nitidus-mail.**
`MailBackend` gains `create_folder`, `delete_folder`, `rename_folder`;
`MailCommand::{CreateFolder, DeleteFolder, RenameFolder}` route through the
actor, replying with a refreshed `MailEvent::Folders` on success and
`JobFailed` on error. Maildir impl in `maildir/folders.rs`: display-path →
Maildir++ dot-name encoding, create = mkdir `cur/new/tmp`, rename =
directory rename including child dot-dirs, delete = refuse unless
`cur`/`new` are empty and no child folders exist; INBOX rename/delete
refused. Mock backend gets in-memory equivalents. Unit tests for the
encoding and guards; engine integration tests for the command round-trips.

**Phase 2 — sidebar UI in the nitidus bin.**
`nitidus-ui-kit::layout` gains `sidebar_layout()` and `main_layout()`
(content minus `SIDEBAR_WIDTH = 24`). New `sidebar/` module: `tree.rs`
(pure: account sections, `/`-split nesting with synthetic parents, INBOX
first, collapse set, two-source unread counts — fully unit tested),
`render.rs` (rows + selection highlight + unread badges), `mod.rs`
(`SidebarPlugin`, `SidebarState`, widget spawn, refresh system, layout swap
on visibility change), `ops.rs` (focus, motion, collapse, select-folder
with outgoing-scan cancel + pager close + focus return). Router gains the
`[sidebar]` context gate on `SidebarState.focused`; keymap defaults per
§2.6; `Action` additions with parser strings. `FolderChanged` handling
re-issues `ListFolders`. Integration tests: tree over a real multi-folder
maildir, folder switch triggers lazy sync and cancels the outgoing scan,
badge reflects optimistic flag edits.

**Phase 3 — command-line folder ops + smoke.**
`:folder-create <path>`, `:folder-rename <new-path>`, `:folder-delete`
wired through the command parser acting on the sidebar selection; sidebar
`c`/`r`/`D` pre-fill the command line. Integration tests for create/
rename/delete round-trips including the non-empty-delete refusal and
deleted-while-viewed fallback to INBOX. Pty smoke on the kenianbei corpus:
tree renders all 11 labels under the account section, collapse works,
switching to a label folder lazily syncs and fills the index, `b` gives the
index full width. Record results in §5/§6.

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **204 passed, 0 failed**
  (was 182 pre-feature: +6 maildir folder-op unit tests, +2 engine
  round-trip tests, +4 tree unit tests, +6 sidebar integration tests,
  +3 parser tests; 3 existing tests updated for changed behavior).
- Integration coverage: two-source unread counts, folder switch with lazy
  sync, badge follows optimistic reads, create/rename/empty-delete round
  trip, non-empty-delete refusal with warning, deleted-while-viewed
  reanchor to INBOX.
- Pty smoke against the live corpora (both accounts): sidebar shows the
  kenianbei section with `INBOX (2)`, all 11 labels, `▸ [Gmail]` collapsed
  by default, then the norman.kerr.dev section; statusline reads
  `mail ⋅ INBOX ⋅ 2/2 ⋅ 1/604`. `Tab j Enter` switched to Condo, which
  lazily synced its real messages into the index. `b` hid the sidebar with
  the index taking the full width, no stale cells.

## 6. Implementation Report

Implemented per plan, with these deviations and notes:

- **Synthetic parents are selectable** (deviation from §2.2's
  "non-selectable"): a collapsed synthetic node like `[Gmail]` must be
  reachable to expand it. Enter on a synthetic row toggles collapse;
  account headers remain non-selectable and the cursor skips them.
- **Tab shadowing.** `<Tab>` was globally bound to `:tab-next`; the index,
  pager, and sidebar contexts now bind it to `:sidebar-focus`, which wins
  by the context-over-global rule. `<BackTab>` still rotates tabs. `<Esc>`
  also returns focus from the sidebar.
- **Command-line prefill.** `:command-line` gained an optional argument:
  the text pre-fills the input (plus a trailing space), which is how the
  sidebar's `c`/`r`/`D` bindings stage `:folder-create` / `:folder-rename`
  / `:folder-delete` for review before Enter. `Action::OpenCommandLine`
  now carries that string; `CommandLineState::prefill` is new.
- **Rename-while-viewed** falls back to INBOX through the same
  `Folders`-event reanchor as deletion (the old folder id vanishes from
  the list); the renamed folder is one sidebar selection away.
- **`command.rs` split from `action.rs`**: the command table had pushed
  `action.rs` past the 300-line limit, so the vocabulary (specs, parser,
  completion) moved to its own module; `action` re-exports
  `parse_command`/`complete_command` so callers are unchanged.
- `IndexWidget`/`PagerWidget` are now public so the sidebar's visibility
  system can swap their `WidgetLayout`s; `EnginePlugin` initializes
  `IndexView` for headless harnesses.
- The statusline's left segment now includes the viewed folder's display
  name (small addition in scope — folder switching is invisible without
  it).

Follow-ups: a config key for sidebar width/visibility (waits on the
settings batching noted in R2), unread-count refresh for non-watched
backends (IMAP will push its own counts), and mouse clicks on sidebar rows
(mouse pass is roadmap item 27).

## 7. Testing and Cleanup
