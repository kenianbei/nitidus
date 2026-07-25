# feature - Batch Operations - v1

Roadmap 1f.25: marking — visual ranges, sticky marks, whole threads — batch
flag/move/delete over the marked set, and the long-promised `z` undo for
destructive index actions (deferred here from the delete feature, which noted
"the delayed-op machinery belongs with marking").

## 1. Current Design

- **Every index verb is single-target.** `flag_selected`, `delete_selected`, and
  `move_selected` all resolve exactly one envelope (the pager's open message or
  the index selection). There is no mark state anywhere — `IndexView` holds
  selection, sort, folds, limits, search.
- **Destructive ops dispatch immediately.** `index/remove.rs` removes the row
  from `MailStore` optimistically and sends the engine command
  (`MoveMessage`/`DeleteMessage`) in the same call — nothing to undo, ever; the
  design doc for delete called the trash folder the undo. Permanent deletes
  inside trash are gated by a y/n prompt only.
- **Undo machinery exists once, for send**: the outbox holds queued sends for
  `SEND_DELAY` (10s) before delivery, and `z`/`:undo-send` removes the newest
  unsubmitted entry. That is the pattern the roadmap wants generalized: delay
  the irreversible part, keep the optimistic UI instant, let `z` cancel.
- **Restoring an optimistically removed row is possible**: removal keeps no copy
  today, but `EnvelopeSummary` is cheap to clone, and the store's
  stamp/reconcile design means a restored row is validated by the next scan
  regardless.
- **Threads are known per row**: `OrderEntry` carries depth, and the JWZ thread
  rows group contiguously, so "the selection's whole thread" is a computable row
  range in threaded mode (and a `references`-walk otherwise).
- **Keys free in the index context**: `v`, `<Space>`, `t`, `x`, `Esc` are all
  unbound (`t` is free; `T` is `:threads`). `z` is `:undo-send` — the roadmap
  wants `z` to become the universal undo.
- Rendering has a flags cell per row (`N`/`F`/`R`/`D` markers) and, since 1f.24,
  span-level styling — room for a mark indicator both as a character and a row
  style.

## 2. Proposal

1. **Mark state** in `IndexView`: `marked: HashSet<EnvelopeId>` plus a
   `visual_anchor: Option<usize>`. Three ways in:
   - `<Space>`/`:mark` — toggle the selection's mark and advance one row (mutt's
     tag rhythm, for non-contiguous sets);
   - `v`/`:visual` — anchor a visual range at the selection; motions extend it
     (anchor..cursor rendered marked live); any batch verb applies to the range;
     `v` again or `Esc` drops the anchor;
   - `t`/`:mark-thread` — toggle marks on every row of the selection's thread
     (threaded mode: the contiguous thread block; flat mode: the message plus
     everything sharing its reference chain).
   - `Esc`/`:unmark-all` — clear all marks and the anchor. The statusline shows
     `3 marked` while any exist.
2. **Batch verbs**: when marks exist (sticky or visual), the existing verbs
   apply to the whole marked set instead of the selection — `d` (trash, or
   permanent-with-confirm inside trash), `:move <folder>`, `F` flag, `u`
   read-toggle. Toggles resolve per message (each toggles its own state). After
   a batch verb, marks clear. No marks → exactly today's single-target behavior.
3. **Staged destructive ops + `z` undo.** Deletes and moves (single _and_ batch)
   stop dispatching immediately: rows leave the store optimistically as today,
   but the engine commands are **staged** for `OP_DELAY` (5s, test-shrinkable
   resource like `SendDelay`). `z` cancels the newest staged op (LIFO),
   restoring its rows to the store; the timer expiring dispatches for real.
   Statusline: `deleted 3 — z undoes`. Quitting flushes staged ops immediately
   (dispatch, never drop). Flag/read toggles stay immediate — they are their own
   undo.
4. **`z` becomes `:undo`**: it cancels the newest staged index op first and
   falls back to `:undo-send`'s behavior when none are staged (the send window
   rarely overlaps a delete window; LIFO across both would surprise).
   `:undo-send` remains as an explicit command.
5. **Rendering**: marked rows get a `*` in the flags cell and the theme's info
   style; the visual range renders identically while extending.

Out of scope: undo for flag writes (toggle again), undo history deeper than the
staged queue, cross-folder mark persistence (marks clear on folder switch),
marking in the pager, and the phase 2 pattern-driven `:tag`.

## 3. Discussion

### 3.1 R1 Questions

1. **Marking model.** Sticky `<Space>` marks + `v` visual ranges + `t`
   thread-marks, `Esc` clears, batch verbs consume and clear. Confirm the model
   and the keys (`v`, `<Space>`, `t`, `Esc` are all free today)?
2. **Undo semantics.** Destructive ops stage for 5 seconds (rows vanish
   instantly, engine commands wait), `z` cancels newest-first, quit flushes. OK
   — and is 5s the right window (send uses 10s)?
3. **`z` unification.** `z` = `:undo` (staged index ops first, then queued
   sends). Confirm?
4. **Batch scope.** `d`, `:move`, `F`, `u` go batch; anything else you want
   marked-aware in v1 (e.g. `A` add-contact over marks — proposed no)?
5. **Folder switch with marks.** Proposal: switching folders clears marks (they
   are per-view working state, not durable tags — tags are phase 2). Confirm?
6. **Smoke.** You drive: mark a few with `<Space>`, `v`-range a block, `t` a
   thread, batch-flag and batch-delete, `z` inside the window (rows return), let
   one expire (rows purge server-side), quit mid-window to verify the flush. OK?

### 3.2 R1 Answers

I think this might be a good time to do some more work on how hotkeys are set
up. Can you open a new refactor contrib, that researches yazi, lazygit, and
helix, and tries to be in line with those apps, with the caveat that they are
slightly different apps. I think yazi does a really good job on bulk ops.

1. Should be good, depending on outcome of hotkey rethink.
2. all good
3. confirm
4. confirm
5. ok, depending on outcome of hotkey rethink.

### 3.3 R2 — resolution after refactor-keymap-v1

The keymap rethink shipped and merged (`refactor-keymap-v1`): tabs moved to
`[`/`]` + `1`/`2`, `D` became permanent-delete, `*` flag, `,` sorts, and
arrow pane navigation landed. It **reserved exactly the keys this feature
proposed** — `<Space>` mark, `v` visual, `t` mark-thread, `Esc` clear, `z`
undo — matching yazi's selection model (`Space` toggle+advance, `v` visual,
`Esc` cancel). §3.2's conditional answers resolve to yes as proposed; the
batch verbs additionally cover `D` (batch permanent delete, confirmed) and
`*` (batch flag) under their new names.

## 4. Plan

Each phase leaves the workspace compiling with clippy clean and tests green.

1. **Marks.** `IndexView` gains `marked: HashSet<EnvelopeId>` and
   `visual_anchor: Option<usize>`; `index/marks.rs` with the verbs
   (`:mark` toggle+advance, `:visual`, `:mark-thread`, `:unmark-all`),
   visual-range resolution (anchor..cursor over visible entries),
   folder-switch clearing, and the `n marked` statusline segment.
   Marked rows render `*` in the flags cell with the info style
   (visual range included, live). Keys: `<Space>`, `v`, `t`, `Esc`.
   E2e tests for each entry path and the render flag.
2. **Staged destructive ops.** `index/staged.rs`: a `StagedOps`
   resource (LIFO queue of pending delete/move commands with their
   removed `EnvelopeSummary` copies), an `OpDelay` resource (5s,
   test-shrinkable), a tick system dispatching expired ops, flush on
   `AppExit`, and `MailStore::restore_envelopes`. `remove.rs`
   dispatch routes through staging; statusline `deleted N — z
   undoes`. `z` becomes `:undo` (staged first, then undo-send).
   Tests: undo restores rows, expiry dispatches, LIFO order, flush on
   exit, undo-send fallback.
3. **Batch verbs.** When marks exist, `d`/`D`/`:move`/`*`/`u` resolve
   the marked set (order: current sort), stage or apply per message,
   clear marks after. Confirm prompts state the count ("Delete 3
   permanently?"). Tests: batch trash + undo restores all, batch
   flag toggles per message, batch move, marks cleared.
4. **Verification & smoke handoff.** Clippy + full run with counts;
   Norman's checklist per §3.1-6. Fill §5–§7.

## 5. Verification

- `cargo clippy --workspace --all-targets`: zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace`: **404 passed, 0
  failed** (was 392 at branch start).
- New coverage:
  - marks (e2e): `<Space>` toggles and advances with the statusline
    count, visual ranges follow the cursor and drop on `v`, `t`
    toggles the whole reference chain (loner untouched), `Esc`
    clears, folder switches clear, `batch_ids` yields visible order;
  - staged undo (e2e over a real maildir): the row leaves the view
    instantly while nothing reaches disk, `z` restores (single and
    whole-batch), LIFO across two staged ops with the undo-send
    fallback after, expiry dispatches (zero-delay), **AppExit flushes
    staged ops to the backend**;
  - batch verbs (e2e): batch trash + one-undo-restores-all, expiry
    moves every file, `D` confirms with the count and purges with no
    trash copy, `:move` files every marked row, batch flag toggles
    exactly the marked pair and consumes the marks.
- Live smoke (Norman): **PASSED** — sticky and visual marking with
  the statusline count, thread toggle, batch flag, batch trash with
  in-window `z` restore and post-expiry server verification, the
  quit-flush round trip, and the counted `D` confirm — all as
  expected on his live INBOX.

## 6. Implementation Report

- Marking went into `IndexView` (sticky set + visual anchor) with one
  consumption point: `batch_ids` resolves sticky ∪ visual in visible
  order, and every batch verb goes through it — no marks means the
  single-target paths are byte-for-byte the old behavior.
- Staging generalized the outbox's undo pattern: a `StagedOps` LIFO
  queue holding engine commands plus the removed row clones, an
  `OpDelay` (5s, test-shrinkable) window, a tick dispatcher, and a
  `Last`-schedule `AppExit` flush so quitting can never drop a staged
  op. A whole batch is one staged op — one `z` restores all of it.
  `MailStore::restore_envelope` re-inserts warm-stamped rows, so the
  next completed scan reconciles them against the server either way.
- `z` = `:undo`: staged ops first (newest), then the send queue —
  `:undo-send` survives as the explicit command.
- Two latent bugs fixed en route: `resolve_selection` indexed
  envelopes through one-frame-stale entries (the same family as
  1e.21's delete panic — now lenient), and the folder-change watcher
  originally tripped bevy change detection every frame (reads now go
  through `as_ref`).
- Follow-ups: mark-aware `A`/add-contact if ever wanted (declined in
  R1), pattern-driven `:tag` and whole-thread limits in phase 2, and
  the flag-write path could stage too if flags ever stop being their
  own undo.

## 7. Testing and Cleanup

- Cleanup scope: the branch diff vs main. Comments state invariants
  (one staged op per batch, warm-stamp reconciliation, marks as
  per-folder working state, the as_ref change-detection guard); no
  dead code — clippy silent, every helper has callers. `remove.rs`
  sits at 277 lines, inside the budget.
- No smoke artifacts: the smoke ran on live data.
- Final verification after the smoke:
  `cargo clippy --workspace --all-targets` zero warnings;
  `CARGO_INCREMENTAL=0 cargo test --workspace` **404 passed, 0
  failed** (suite counts confirmed present).
