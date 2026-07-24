# feature - Threading - v1

Roadmap item 1b.9. Conversation threading for the index: hand-rolled JWZ over
message-id/references, a flat `ThreadRow` display list computed off the UI
thread, collapse/expand, jump-to-parent, and the groundwork for server-side
THREAD passthrough when IMAP lands.

## 1. Current Design

- `EnvelopeSummary` is
  `{ id, subject, from_display, from_addr, date_epoch_secs, flags }` — **no
  `Message-ID`, `References`, or `In-Reply-To`**, so nothing can be threaded
  yet. The maildir backend parses a 64KiB header window with mail-parser (which
  exposes `message_id()`, `references()`, `in_reply_to()` accessors, unused).
- The cache schema (v1) mirrors those six fields; `rusqlite_migration`'s ladder
  exists but has only one migration — a v2 would be the first real schema
  migration.
- The index displays a flat permutation (`IndexOrder`, `Vec<u32>` over the
  store's date-desc slice); sorts are stable-sorted identities. Selection is
  identity-based (`EnvelopeId`), motions operate on rows.
- `MailEngine` owns a tokio runtime and an events channel; long work runs as
  jobs with `JobId`s. The UI drains events per frame. There is no facility yet
  for CPU-bound jobs that aren't backend IO.
- `MailBackend` has four methods (folders/scan/fetch/flags); no threading hook.

## 2. Proposal

### 2.1 Envelope identity fields

Extend `EnvelopeSummary` with `message_id: String` (empty when the header is
missing/unparseable) and `references: Vec<String>` — the `References` list with
`In-Reply-To` appended as a fallback last entry when `References` is absent (the
JWZ-recommended reading order). Maildir parse fills both from the existing
header window; the mock backend generates a small reply structure so engine
tests can thread.

Cache schema **migration v2**:
`ALTER TABLE envelopes ADD COLUMN message_id TEXT NOT NULL DEFAULT ''` and
`reference_ids TEXT NOT NULL DEFAULT ''` (newline-joined; message-ids cannot
contain newlines). Existing caches migrate in place — the first live exercise of
the migration ladder. Warm reads populate the new fields.

### 2.2 JWZ in `nitidus-mail::thread`

A pure module, no IO:
`compute_thread_rows(&[EnvelopeSummary]) -> Vec<ThreadRow>` implementing JWZ's
algorithm (containers keyed by message-id, reference chains linked, missing
messages as phantom containers that are pruned, cycles broken, duplicate/empty
ids handled by treating the message as unthreadable root).
`ThreadRow { index: u32, depth: u8, root: u32, has_children: bool }` — indices
into the input slice, flattened depth-first.

Ordering: threads sorted by their **newest** message date, descending (active
conversations first); within a thread, chronological ascending walk (classic
mutt reading order).

### 2.3 Off-thread computation via an engine job

Threading 100k envelopes is tens-of-ms CPU work — too much for a frame. New
engine API:
`MailEngine::compute_threads(account, folder, envelopes: Vec<EnvelopeSummary>, job)`
spawns the pure computation on the engine runtime and emits a new
`MailEvent::Threads { account, folder, job, rows }`. The UI requests a recompute
(debounced to scan-done / store-settled, not per batch) with a snapshot of the
folder slice; superseded jobs are simply ignored on arrival (newest job id wins
— no cancellation machinery needed for a pure function).

### 2.4 Index integration

- `IndexView` gains `threaded: bool` and `collapsed: HashSet<EnvelopeId>` (keyed
  by thread-root envelope id, so collapse state survives re-threads).
  `IndexOrder` gains the thread mode: when threaded rows arrive, the display
  list becomes the `ThreadRow` walk with collapsed subtrees filtered out (roots
  remain, showing a `[n]` collapsed count).
- Rendering: subject column indents by depth with `↳ ` at depth ≥ 1; collapsed
  roots show `[+n]` before the subject. Everything else (flags, date, from,
  selection highlight) unchanged.
- While a re-thread is in flight (or threading data is absent), the index keeps
  showing the previous order — no flicker, eventual consistency within a frame
  or two.
- `:sort` in threaded mode applies to **threads** (by root, using the thread's
  newest message for date/unread/flagged keys); within-thread order is fixed
  chronological.

### 2.5 Commands and bindings

| command       | action                            | index binding |
| ------------- | --------------------------------- | ------------- |
| `:threads`    | toggle threaded mode              | `T`           |
| `:fold`       | toggle collapse of current thread | `za`          |
| `:fold-all`   | collapse every thread             | `zM`          |
| `:unfold-all` | expand every thread               | `zR`          |
| `:parent`     | jump to thread parent             | `P`           |

Cursor motions operate on visible rows (collapsed subtrees are skipped
naturally). Flag ops stay single-message this item; thread-scoped operations
(mark thread read, etc.) arrive with the ops that need them (delete/archive,
1c+).

### 2.6 Server THREAD passthrough hook

Deferred to the IMAP backend item: the seam already exists cleanly —
`MailEvent::Threads` is backend-agnostic, so an IMAP `THREAD` response only
needs to emit the same event instead of running JWZ. No trait method is added
now (avoiding a speculative default-None method).

## 3. Discussion

### 3.1 R1 Questions

1. **Threaded by default?** Proposal: threading **on** by default (it is the
   neomutt/gmail mental model), `:threads` / `T` toggles to flat. Or start
   flat-by-default until threading has soaked?
2. **Subject-grouping (JWZ step 5).** Group same-normalized-subject
   (`Re:`-stripped) messages that lack reference links into one thread?
   Gmail-ish but notorious for false positives (`"hi"` threads). I propose
   **strict references-only** for v1, subject-grouping later as an opt-in.
   Confirm?
3. **Computation site** (§2.3): engine-runtime job over a snapshot of the folder
   slice (clone cost ~20MB at 100k, only on scan-done). Alternatives: threading
   inside the actor as batches stream (incremental but stateful and coupled), or
   synchronous in-frame with a size cutoff. I prefer the engine job; confirm?
4. **Ordering** (§2.2): threads by newest-message date desc, chronological
   within thread; `:sort` re-keys the thread roots. Confirm?
5. **Bindings** (§2.5): `T`/`za`/`zM`/`zR`/`P` (vim-fold flavored). Preference
   changes welcome — aerc uses `zz`-style folds too, neomutt uses `<Esc>v`.
6. **Migration duty of care**: v2 migrates existing `mail.db` in place;
   cache-tier contract means a failed migration falls back to
   delete-and-recreate (already implemented). Any concern with silently
   rebuilding the cache on migration failure, or is a log line enough?

### 3.2 R1 Answers

1. Let's do flat by default.
2. confirm
3. confirm
4. confirm
5. vim-fold flavored is fine
6. log line is enough

## 4. Plan

Design deltas from R1: flat by default (Q1); `ThreadRow` carries
`EnvelopeId`s (plus `parent`) rather than slice indices, so rows stay
valid while the store mutates between re-threads — vanished ids simply
drop out of the display list.

**Phase 1 — envelope identity + cache v2** (compiles, tests green):

1. `EnvelopeSummary` + `message_id: String`, `references: Vec<String>`;
   maildir parse fills both (References, In-Reply-To fallback); mock
   backend generates a reply structure.
2. Cache migration v2 (two ADD COLUMNs), writer upserts, warm reads.
3. Tests: header parse with references; v1 database (replicated SQL +
   `user_version=1`) opens, migrates to v2, and preserves rows.

**Phase 2 — JWZ** (`nitidus-mail/src/thread.rs`, pure):

1. `ThreadRow { id, parent: Option<EnvelopeId>, root, depth,
   has_children }`; `compute_thread_rows(&[EnvelopeSummary])` —
   containers by message-id, reference-chain linking (cycle-safe),
   phantom pruning, threads by newest date desc, chronological DFS
   within.
2. Tests: linear chain, branch, missing-parent promotion, cycle,
   duplicate ids, empty ids, ordering, orphan singletons.

**Phase 3 — engine job**: `MailEngine::compute_threads(account, folder,
envelopes, job)` on the runtime → new `MailEvent::Threads`; engine test
over mock reply data.

**Phase 4 — index integration** (bin):

1. `store.rs`: `ThreadSet` resource (account, folder, latest job, rows)
   filled by the drain; `MailStore::position_of` for id→index lookup.
2. Drain: on scan-done for the viewed folder in threaded mode, snapshot
   + `compute_threads` (latest job wins).
3. `IndexOrder` entries become `{ index, depth, collapsed_children }`;
   threaded build walks `ThreadRow`s, resolves ids, filters collapsed
   subtrees; sort re-keys threads by root using newest-message keys.
4. `IndexView` + `threaded`, `collapsed`; actions `:threads`(`T`),
   `:fold`(`za`), `:fold-all`(`zM`), `:unfold-all`(`zR`),
   `:parent`(`P`) (a `Motion`); render depth indent `↳ ` and `[+n]`.
5. Tests: threaded order build with collapse, motions skip collapsed,
   parent jump, toggle triggers compute and event round-trips into a
   threaded index (mock backend), flat mode untouched.

**Phase 5 — verification**: clippy, full workspace tests with counts,
isolated `cargo build -p nitidus-mail` + no-bevy check, pty smoke with a
reply-fixture maildir (thread indentation and folds visible via pyte),
cache-migration check against the existing real `mail.db`.

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **144 passed, 0 failed**
  (was 128): nitidus unit 88 + index integration 5, nitidus-contacts 1,
  nitidus-mail unit 14 (7 JWZ) + cache 7 + engine 8 + maildir 6,
  nitidus-ui-kit 15.
- Isolated `cargo build -p nitidus-mail` clean; `cargo tree` no-bevy
  check holds.
- pty smoke (90×24, maildir fixture: 2024 root + two dated replies via
  `References` + one unrelated message), pyte-replayed:
  - After `T`: thread first (keyed by its newest reply), rendered
    `Project kickoff` / `↳ Re:` / `  ↳ Re:` (depths 0/1/2), lone message
    after; the flat selection followed its message identity from row 1
    to threaded row 3 (`3/4`).
  - After `za`: `[+2] Project kickoff` with both replies hidden, cursor
    moved to the root (`1/4`).
- **Live migration**: the 1b.7-era scratch cache (`user_version=1`,
  three gmail rows) opened by the new binary migrated in place to
  `user_version=2` with all rows preserved and empty identity columns
  (correct — those fixtures carry no Message-ID header).

## 6. Implementation Report

Implemented per plan with the R1 deltas (flat by default, id-based
`ThreadRow`s). Notable specifics:

- **Re-thread trigger** is store-structural, not drain-coupled:
  `FolderEnvelopes` tracks a `structure_generation` bumped only when the
  id set changes (adds/prunes — never flag edits), and
  `refresh_threads` requests a recompute when the viewed folder's
  generation is unaccounted for and no scan is in flight. Flag toggles
  therefore never clone the folder for a pointless re-thread.
- **`ThreadSet`** (store.rs) is the arbiter of job races: `begin` /
  `accept` keyed by (scope, job, generation); stale or out-of-scope
  results are dropped on arrival. Threaded mode falls back to the flat
  list until the first rows land (a frame or two).
- **JWZ pruning**: an empty root with several real children promotes
  the oldest child to root and re-parents the rest (mutt's pseudo-root
  grouping without a dummy row).
- **Thread-unit sorting deviation from §2.4**: `unread`/`flagged` key on
  *any* message in the thread matching (not the newest message's flags)
  — a thread with one unseen reply sorts as unread, which is what the
  keys are for.
- Collapsing a thread moves the cursor to its root so the selection
  never silently sits on a hidden row; `zM`/`zR` set/clear the whole
  fold set; `P` walks `ThreadRow.parent`.
- The mock backend now generates reply chains (threads of three), which
  the engine and index integration tests thread end-to-end.
- `index/thread_view.rs` owns display-order construction (both modes +
  the two systems); `mod.rs` stays plugin + window build.

Follow-ups for later items:

- Subject-grouping (JWZ step 5) as an opt-in, per Q2.
- Server THREAD passthrough when the IMAP backend lands (emit the same
  `Threads` event).
- Each recompute clones the folder slice (~20MB at 100k); revisit only
  if profiling shows it matters — it runs off-frame.
- `resolve`/`sort_nodes`/`newest_date` recurse per thread depth; fine
  for real mail (depth ≪ 1000), worth an iterative rewrite only if a
  pathological corpus shows up.

## 7. Testing and Cleanup
