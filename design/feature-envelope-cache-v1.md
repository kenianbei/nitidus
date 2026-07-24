# feature - Envelope Cache - v1

Roadmap item 1b.7. A persistent SQLite envelope cache plus the in-memory
`MailStore` resource it hydrates, so nitidus starts warm (folders and envelope
lists visible immediately) and mail events finally land somewhere instead of the
debug log. This is the data layer 1b.8 (virtualized index) draws from.

## 1. Current Design

- `nitidus-mail` produces `MailEvent`s: `Folders`,
  `EnvelopeBatch { batch, job, done }`, `Message`, `FolderChanged`,
  `Connection`, `JobFailed`. `EnvelopeSummary` is
  `{ id, subject, from_display, from_addr, date_epoch_secs, flags }`.
- The bevy-side drain (`crates/nitidus/src/engine.rs`) consumes ≤64 events per
  frame but only routes `Connection` into `EngineStatus`; everything else hits
  `tracing::debug!` ("unrouted until MailStore") — the follow-up this item
  closes.
- Initial sync is INBOX-only (`register_maildir` sends one `ListFolders` + one
  `SyncEnvelopes`); `FolderChanged` from the maildir watcher is emitted but
  nothing re-syncs.
- Nothing persists between runs: every start is a cold full scan.
- `dirs.rs` resolves state/config dirs only; there is no cache dir helper.
- `documentation/persistence.md` §5 specifies the target:
  `~/.cache/nitidus/mail.db` via rusqlite (`bundled`), WAL +
  `synchronous=NORMAL` + `foreign_keys=ON` + `busy_timeout`,
  `PRAGMA user_version` migrations with downgrade refusal, one transaction per
  500-envelope batch, DB owned by the mail side (UI reads go through memory,
  never SQL).

## 2. Proposal

Two halves: a persistent cache in `nitidus-mail` (no bevy, per the crate
invariant) and a `MailStore` bevy resource in the bin crate, both fed from the
same event stream.

### 2.1 `nitidus-mail::cache`

New module `crates/nitidus-mail/src/cache/` (`schema.rs`, `writer.rs`,
`mod.rs`):

- `MailCache::open(path) -> Result<MailCache, CacheError>` — opens/creates the
  DB, applies pragmas, runs migrations via `rusqlite_migration`
  (`user_version`-based); a `user_version` newer than the binary knows is
  refused (downgrade protection, surfaced as `CacheError::NewerSchema`).
- Schema v1, deliberately minimal (no FTS5, labels, or harvested_addrs until the
  features that need them exist):
  - `folders(account, id, name, unread, total, PRIMARY KEY(account, id))`
  - `envelopes(account, folder, id, subject, from_display, from_addr, date_epoch_secs, flags, seen_job, PRIMARY KEY(account, folder, id))`
- **Warm read API** (used once at startup, before the writer starts):
  `load_folders(account)` and `load_envelopes(account, folder)` returning the
  same `FolderMeta`/`EnvelopeSummary` types the engine emits.
- **Writer thread**: rusqlite `Connection` is not `Sync`, so after the warm read
  the connection moves onto one dedicated thread consuming `CacheOp`s from a
  bounded flume channel. `MailCache` becomes a cheap cloneable handle with
  non-blocking `record(&MailEvent)` that maps events to ops: `Folders` → upsert
  folder rows, `EnvelopeBatch` → one transaction per batch upserting rows
  stamped with the batch's `JobId`.
- **Stale-row reconciliation**: a scan is a full folder listing, so when a batch
  arrives with `done = true`, the writer deletes rows in that folder whose
  `seen_job` differs from the finishing job — deleted/moved mail disappears from
  the cache without tombstones. (This is the maildir analogue of the UIDVALIDITY
  drop-and-resync discipline; IMAP cursor columns are added when the IMAP
  backend lands.)
- Cache failures are never fatal: `open` failure attempts delete-and-recreate
  once (it is cache-tier by contract); if that also fails, or the schema is from
  a newer nitidus, the app runs cacheless with a startup notice. Writer-side
  errors log and drop the op.

### 2.2 `MailStore` resource + drain routing (bin crate)

- `MailStore`: `folders: BTreeMap<AccountId, Vec<FolderMeta>>` and
  `envelopes: HashMap<(AccountId, FolderId), FolderEnvelopes>` where
  `FolderEnvelopes` keeps a date-sorted `Vec<EnvelopeSummary>` plus an id→index
  map. Change detection via `ResMut` drives 1b.8 redraws.
- Drain routing (replacing the debug-log arm): every event is first `record`ed
  to the cache handle (if present), then applied to `MailStore` — `Folders`
  replaces the account's folder list, `EnvelopeBatch` upserts live and prunes
  non-matching `seen_job` entries on `done` (same reconciliation as the DB, so
  screen and disk agree), `FolderChanged` triggers a re-sync `SyncEnvelopes` for
  that folder, superseding any in-flight scan of the same folder via
  `Cancel(job)`.
- **Sync orchestration (lazy)**: INBOX is synced eagerly at registration as
  today; other folders sync on first view. A `SyncTracker` resource records
  in-flight jobs and folders synced this session, and exposes the
  ensure-synced entry point 1b.8's folder switching will call. `FolderChanged`
  re-syncs only folders already synced or in flight this session — an unviewed
  folder's cache stays stale until first view triggers its scan anyway.
- **Warm start**: `run()` opens the cache (path from a new `dirs::cache_dir()`,
  honoring the XDG cache strategy), bulk-loads folders + envelopes into the
  initial `MailStore`, inserts both resources, then registration kicks off live
  scans that reconcile the warm data.
- Statusline: no visual change this item beyond existing notices; the index
  screen (1b.8) is the consumer.

### 2.3 Dependencies

`rusqlite 0.40` (`bundled`) and `rusqlite_migration 2.6` added to workspace
deps and `nitidus-mail`. The writer thread uses the existing flume dependency.

## 3. Discussion

### 3.1 R1 Questions

1. **Cache placement / event flow.** Proposal: the cache lives in `nitidus-mail`
   (writer thread owns the only `Connection`), but it is _fed by the bevy drain_
   — the drain stays the single consumer of the event channel and tees each
   event into the cache handle before updating `MailStore`. The alternative is
   teeing inside the engine (cache sees events even if the UI stalls, but adds a
   second consumer path and engine-owned lifecycle). I prefer drain-side teeing
   for the single event path; confirm?
2. **Multi-folder sync at startup.** Proposal: sync _all_ discovered folders
   each start, INBOX first, serially per account. For large accounts this is
   more IO than lazy per-folder-on-view, but it makes folder switching instant
   in 1b.8 and the maildir scan is cheap. Alternative: INBOX eagerly, others on
   first view. Which?
3. **Failure policy.** Open failure → delete + recreate once → else run
   cacheless with a notice. Newer-schema DB → run cacheless with a notice (no
   auto-delete of a newer nitidus's data). Acceptable?
4. **Migrations.** Hand-rolled `user_version` ladder (no new dep) vs the
   `rusqlite_migration` crate. persistence.md allows either; I lean hand-rolled
   at this schema size.
5. **Schema scope.** v1 tables are only `folders` + `envelopes` — FTS5, labels,
   harvested_addrs, carddav_state, IMAP cursor columns all deferred to the items
   that use them. Confirm this trim (it deviates from the fuller sketch in
   persistence.md §5 on purpose)?
6. **Re-sync supersede semantics.** `FolderChanged` during an in-flight scan of
   the same folder cancels it and starts fresh (newest wins). Bursts are already
   coalesced by the watcher's 500ms window, so no extra debounce on the drain
   side. Confirm?

### 3.2 R1 Answers

1. confirm
2. INBOX eagarly, others on first view
3. acceptable
4. rusqlite_migration
5. confirm
6. confirm

## 4. Plan

Proposal §2 updated per R1: lazy folder sync (Q2) and `rusqlite_migration`
(Q4) folded in.

**Phase 1 — `nitidus-mail::cache`** (workspace compiles, tests green):

1. Workspace deps: `rusqlite` (bundled), `rusqlite_migration`; add both to
   `nitidus-mail`. `Flags::bits()`/`Flags::from_bits()` for DB storage.
2. `cache/schema.rs` — migration list + open-time pragmas; `cache/mod.rs` —
   `MailCache::open`, warm reads (`load_folders`, `load_envelopes`),
   `CacheError`; `cache/writer.rs` — `CacheOp`, dedicated writer thread,
   `CacheWriter` handle with `record(&MailEvent)` and `close()` (joins the
   thread so tests and shutdown are deterministic).
3. `tests/cache.rs` — migrate-on-open (`user_version` at latest),
   record→close→reopen roundtrip (folders, envelopes, flags), prune-on-done
   removes stale rows, newer-schema refusal.

**Phase 2 — bin store + routing** (workspace compiles, tests green):

1. `dirs::cache_dir()`; new `store.rs` — `MailStore`, `FolderEnvelopes`
   (date-desc sorted + id index + job stamps), `SyncTracker`; unit tests.
2. New `bootstrap.rs` — cache-open policy (recreate once, cacheless fallback
   with notices), warm load into `MailStore`, `register_accounts` moves here,
   returns an `EngineSetup` bundle; `run()`/`build_app` rewired to take it.
3. `engine.rs` drain routing — tee to cache, apply to store, tracker updates,
   `FolderChanged` → `Cancel` + fresh `SyncEnvelopes`, `JobFailed` → status
   warning.
4. Tests: mock-driven drain fills `MailStore`; live-maildir end-to-end
   (deliver a message → watcher → resync → store grows); warm-start
   hydration from a pre-populated cache; corrupt-DB recreate and
   newer-schema cacheless fallbacks.

**Phase 3 — verification**: `cargo clippy --workspace`,
`CARGO_INCREMENTAL=0 cargo test --workspace` with pass counts, isolated
`cargo build -p nitidus-mail`, `cargo tree -p nitidus-mail` no-bevy check,
pty smoke run (cold, then warm start showing hydrated state).

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **109 passed, 0 failed**
  (was 94): nitidus 68, nitidus-contacts 1, nitidus-mail unit 7 +
  cache 5 + engine 7 + maildir 6, nitidus-ui-kit 15.
- Isolated `cargo build -p nitidus-mail` — clean (guards against feature
  unification masking).
- `cargo tree -p nitidus-mail | grep -i bevy` — no matches; the no-bevy
  invariant holds with rusqlite added.
- pty smoke test (`script` + `stty rows 24 cols 80`, isolated
  `NITIDUS_CONFIG_DIR`/`XDG_CACHE_HOME`/`XDG_STATE_HOME`, real maildir
  with two messages):
  - Cold run: clean exit; `mail.db` created containing
    `folders: (local, INBOX, unread 2, total 2)` and both envelopes with
    subjects, flags, and `seen_job` stamps; no leftover `-wal` sidecar.
  - Warm run after deleting one message on disk: clean exit, statusline
    `mail ⋅ 1/1`, and the cache afterwards holds only the surviving
    message under a new `seen_job` — proving warm hydration and
    live-rescan pruning agree end to end.
- Toolchain note: `rusqlite_migration 2.6` requires rustc ≥1.95;
  `rustup update stable` moved the toolchain 1.93.1 → 1.97.1 (2.5 was
  incompatible with rusqlite 0.40's libsqlite3-sys).

## 6. Implementation Report

Implemented as planned; the notable specifics:

- **`nitidus-mail::cache`** (`schema.rs`, `writer.rs`, `mod.rs`):
  `MailCache::open` applies the WAL pragma set and runs
  `rusqlite_migration` (STRICT tables, one v1 migration); its
  `DatabaseTooFarAhead` error maps to `CacheError::NewerSchema`.
  `Flags` gained `bits()`/`from_bits()` (masked to known bits) for DB
  storage. After warm reads the connection moves into a dedicated
  writer thread (`into_writer`); `CacheWriter::record` is non-blocking
  and `close()` joins deterministically — used by tests and clean app
  exit (`run()` removes the resource after the bevy loop ends and
  closes it).
- **Folder replacement** also deletes envelopes of dropped folders, so
  a renamed/removed maildir folder cannot leave orphan rows.
- **`store.rs`**: `MailStore` (date-desc sorted envelope lists with id
  index and job stamps, reconciled identically to the DB) and
  `SyncTracker` (in-flight jobs + synced-this-session set; a superseded
  job's stray `done` cannot mark a folder synced). Warm rows are
  stamped `JobId(0)` so any live scan's completion prunes them.
- **`bootstrap.rs`** (registration moved out of `engine.rs`):
  `bootstrap()` returns an `EngineSetup` bundle; `request_sync` is the
  single sync entry point (cancel-then-rescan) shared by eager INBOX
  registration, `FolderChanged` re-syncs, and 1b.8's future
  first-view path. Cache-open policy implemented per Q3, including
  `-wal`/`-shm` sidecar removal on recreate.
- **`engine.rs` drain** now routes everything: tee to cache first, then
  `Folders`/`EnvelopeBatch` into the store, `FolderChanged` →
  `request_sync` (only for tracked folders — lazy contract),
  `JobFailed` → tracker cleanup + statusline warning. The 1b.6
  "unrouted until MailStore" follow-up is closed; only `Message`
  remains unrouted (pager, 1b.10). System params grew past four, so
  they are grouped in a `#[derive(SystemParam)] MailRouting` struct.
- End-to-end regression test: external delivery into a watched maildir
  re-syncs into `MailStore` (watcher → `FolderChanged` → resync →
  store), covering the full change-propagation path.

Follow-ups for later items:

- On-exit cache close only runs on clean exit; a crash loses at most
  the in-channel ops (repaired by the next scan) — acceptable by the
  cache-tier contract.
- `MailStore` re-sorts a folder per batch (O(n log n)); revisit in 1b.8
  if profiling shows it matters at 100k envelopes.
- FTS5, labels, IMAP cursor columns, harvested_addrs deferred per Q5.
- `EngineStatus` still counts only connections; richer sync progress
  display belongs to the index screen work.

## 7. Testing and Cleanup
