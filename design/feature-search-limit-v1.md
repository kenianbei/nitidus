# feature - Search and Limit - v1

Roadmap 1f.24, opening the polish phase: incremental `/` search over the index
with match highlighting and next/prev jumps, a stacking `:limit` filter that
narrows the visible rows, and `:clear` to lift it all. Plain text matching only
— the neomutt-class pattern language is phase 2's.

## 1. Current Design

- **Nothing filters and nothing searches.** The index always shows the whole
  folder: `current_envelopes` hands the store's full slice to `refresh_order`,
  which builds the display permutation (`IndexOrder`) keyed on
  `(sort, threaded, fold_epoch)` plus store/thread change detection. Entries are
  `OrderEntry { index, depth, collapsed_children }` — flat sort order, or JWZ
  thread rows computed in the mail actor.
- **Input modes**: `Normal | CommandLine | Prompt`. The command line is the
  incremental-input precedent (live candidate panel, Tab cycling, history);
  prompts got completion in 1e.23. There is no search mode, and neither existing
  mode fits: search must move the index selection live on every keystroke while
  showing what was typed.
- **Rendering is whole-row styling.** `IndexRow` carries plain strings
  (flags/date/from/subject); `row_line` paints one style per row. There is no
  span-level highlight machinery in the index.
- **Selection is identity-based** (`IndexView.selected: Option<EnvelopeId>`) and
  survives re-sorts and re-syncs; the window clamps leniently when rows vanish
  (hardened in 1e.21's delete work) — good news for filtering, which is just
  another way rows vanish.
- **Keys**: `/` is unbound in every context. In the index `n` is free but `N` is
  `:toggle-read` — colliding with the vim/neomutt convention of `n`/`N` for
  search next/prev. `l` and `u` are free in the index.
- The statusline's left segment shows `folder ⋅ position/total` from
  `IndexStatus`; there is no place an active filter announces itself.

## 2. Proposal

1. **Match semantics (shared by search and limit)**: case-insensitive substring
   over subject, from-display, and from-address. One matcher, no operators —
   patterns are phase 2.
2. **`/` incremental search**: a new `InputMode::Search` with its own thin state
   (query + the selection where search began). Every keystroke jumps the
   selection to the first match at-or-after the origin, wrapping; Enter accepts
   (selection stays, query is retained for repeats), Esc restores the origin
   selection. After accepting, search-next/prev repeat over the retained query,
   wrapping, with a statusline nudge when nothing matches. Search operates on
   the _visible_ (possibly limited) rows.
3. **Match highlighting**: the matched substring lights up in subject and from
   cells — while typing and after accepting — until a new search replaces the
   query or `:clear` drops it. This adds span-level rendering to the index rows
   (the one real rendering change).
4. **`:limit <text>`, stacking**: each `:limit` pushes onto a stack; rows must
   match every entry (AND). `refresh_order` filters the entries it already
   builds, so sort order is preserved and the virtualized window is untouched.
   **While any limit is active, threading is suspended** (flat rows) — filtered
   thread trees would show orphaned children with dangling indent structure;
   threads come back on `:clear`. `l` opens the command line prefilled with
   `:limit ` (the wizard-established prefill pattern). The selection survives
   when it matches; otherwise it clamps to the first visible row.
5. **`:clear`**: drops the whole limit stack and the retained search query. The
   statusline left segment announces the state while limited:
   `folder ⋅ limit: foo+bar ⋅ 12/300` (filtered count over folder total).
6. **Keys**: `/` search in index context; `n`/`N` next/prev — which means
   **`:toggle-read` moves from `N` to `u`** (unbound today, reads as "unread").
   `l` limit-prefill, `:clear` typed (or bound later).

Out of scope: pager-internal search (its own follow-up), the pattern/query
language, saved searches as virtual folders (phase 2), search history, and
limiting the sidebar or contacts (index only).

## 3. Discussion

### 3.1 R1 Questions

1. **Matcher.** Case-insensitive substring across subject + from (display and
   address), same matcher for search and limit; anything fancier waits for phase
   2 patterns. Confirm?
2. **The `N` collision.** vim/neomutt reflexes want `n`/`N` for search
   next/prev, but `N` has been `:toggle-read` since the index feature. Proposal:
   move toggle-read to `u` and give search the conventional pair. Alternative:
   keep `N` as is and use `n`/`p` for search. Your call — it's your muscle
   memory.
3. **Limiting suspends threading.** Filtered thread trees mean orphaned children
   under missing parents; v1 shows limited results flat and restores threads on
   `:clear`. (Phase 2's pattern limits can revisit whole-thread matching.) OK?
4. **Highlight lifetime.** Proposal: matches stay highlighted after Enter (they
   mark where `n`/`N` will land) until a new search or `:clear`. Alternative:
   highlight only while the search prompt is open. Confirm the persistent
   version?
5. **Esc during search** restores the selection to where you started; Enter
   keeps you at the match. Confirm?
6. **Smoke.** You drive: `/` into a busy folder, watch the live jump +
   highlight, Enter, `n`/`N` around the wrap, `:limit` twice stacked, watch the
   statusline counts, `:clear` back to threads. OK?

### 3.2 R1 Answers

1. confirm
2. no preference now, but at some point I'd like to rewrite all the hotkeys to
   be closer to yazi,lazygit,helix as these are more modern tuis.
3. ok
4. confirm persistant
5. confirm
6. ok

## 4. Plan

Each phase leaves the workspace compiling with clippy clean and tests green.

1. **Matcher and limits in the order pipeline.** `index/filter.rs`:
   the shared case-insensitive substring matcher plus a match-range
   helper (for highlighting later). `IndexView` gains
   `limits: Vec<String>` and a filter epoch folded into
   `refresh_order`'s key; non-empty limits filter flat entries
   (threading suspended). `:limit` pushes, `:clear` empties, `l`
   opens the command line prefilled. `IndexStatus` carries the
   filtered/total pair and the joined limit text; the statusline
   announces it. Unit + e2e tests for stacking, counts, and the
   threading suspension key.
2. **Search mode.** `InputMode::Search`, `index/search.rs`: `/`
   records the origin selection and enters the mode; keystrokes edit
   the query, jump the selection to the first visible match
   at-or-after the origin (wrapping), and update the live highlight
   query; Esc restores origin and the prior query, Enter accepts.
   `n`/`N` repeat over the retained query with wrap and a "no match"
   nudge. A thin statusline-row widget shows `/query` while the mode
   is open. `:toggle-read` moves `N` → `u`. Tests: live jump, Esc
   restore, accept + wrap both directions, no-match notice.
3. **Highlight rendering.** `IndexRow` gains optional highlight
   ranges for the from and subject cells, computed against the
   displayed strings while a query is retained; `row_line` splits
   spans at the ranges with a highlight style from the theme.
   Renders in every matching row until a new search or `:clear`.
   Render-level tests over a `TestBackend` buffer.
4. **Verification & smoke handoff.** Clippy + full workspace run
   with counts; Norman's checklist (live jump + highlight, `n`/`N`
   wrap, stacked limits with statusline counts, `:clear` restoring
   threads). Fill §5–§7.

## 5. Verification

- `cargo clippy --workspace --all-targets`: zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace`: **387 passed, 0
  failed** (was 378 at branch start).
- New coverage:
  - matcher (unit): subject + both from fields case-insensitively,
    AND-stacking, byte-range finding with the case-fold refusal;
  - limits (e2e): one limit filters with correct filtered/(total)
    statusline counts, stacking ANDs, `:clear` restores, threading
    suspends under a limit and the preference survives the round
    trip;
  - search (e2e): live jump per keystroke, unmatched query returns to
    the origin, backspace re-jumps, Esc restores selection and query,
    Enter accepts, `n`/`N` wrap both directions, the no-search and
    no-match notices, and search operating inside an active limit;
  - rendering (unit): a matching query splits the row into
    prefix/match/suffix spans with the highlight style; unmatched
    rows stay single-span.
- Live smoke (Norman): **PASSED** — live jump with highlight, origin
  restore on mismatch and Esc, `n`/`N` wrap both ways, the no-search
  notice, stacked limits with the filtered/(total) statusline, flat
  limited view with threads restored on `:clear`, and `u` as the new
  toggle-read — all as expected on his live INBOX.

## 6. Implementation Report

- The limit filter lives where the display order is already built:
  `refresh_order` keys on a new filter epoch and filters flat entries,
  so sorting, the virtualized window, and the lenient selection
  anchoring all worked unchanged. Threading suspension is one early
  return in `build_entries`.
- Search is a fourth `InputMode` with deliberately thin state (buffer +
  origin + the query it would restore on Esc). The selection jump
  reuses the identity-based selection — no new scrolling machinery.
- Highlighting operates on the *rendered* row line rather than
  per-cell ranges: one `match_range` over the fitted string, three
  spans out. Matches that truncation cut off simply do not light up;
  non-ASCII case folds that shift byte offsets skip the highlight
  rather than risk mis-slicing.
- `:toggle-read` moved from `N` to `u` to free the conventional
  `n`/`N` search pair.
- Follow-ups: pager-internal search; Norman wants a broader keymap
  modernization pass toward yazi/lazygit/helix conventions at some
  point — that belongs with phase 2's selectable keymap schemes;
  phase 2's pattern language will replace the plain matcher and can
  revisit whole-thread limiting.

## 7. Testing and Cleanup

- Cleanup scope: the branch diff vs main. Comments state invariants
  (threading suspension rationale, the frozen-origin jump contract,
  the case-fold highlight refusal); no dead code — clippy silent,
  every helper has callers. `match_range` was deliberately introduced
  with its consumer in phase 3 rather than sitting unused in phase 1.
- No smoke artifacts: the smoke ran on live data.
- Final verification after the smoke:
  `cargo clippy --workspace --all-targets` zero warnings;
  `CARGO_INCREMENTAL=0 cargo test --workspace` **387 passed, 0
  failed** (suite counts confirmed present).
