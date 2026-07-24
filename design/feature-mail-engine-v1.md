# feature - mail engine - v1

The async spine of the mail client: a dedicated tokio runtime inside
`nitidus-mail` running one actor task per account behind a `MailBackend` trait,
talking to the bevy world through bounded flume channels with job IDs and
cancellation, drained into resources by a `PreUpdate` system. This is roadmap
item 1a.5 — the last foundation item. It ships no real backend (Maildir is 1b.6,
IMAP 1b.12); it ships the machinery those backends plug into, proven end-to-end
with a mock backend under test.

## 1. Current Design

- `crates/nitidus-mail` is the scaffold placeholder from 1a.1: a
  `crate_version()` function, one smoke test, **zero dependencies**, and the
  manifest-documented invariant that bevy must never appear in its dependency
  tree.
- Workspace pins tokio 1 (`rt-multi-thread`), tokio-util 0.7
  (`CancellationToken`), flume 0.12, thiserror 2 — all currently unused.
- The bin crate has no engine wiring; nothing produces or consumes mail data.
  The statusline has left/center/right segments (1a.4) with no connection-state
  display.
- `config::AccountConfig` (1a.3, merged) parses account definitions
  (maildir/imap backend, smtp/sendmail outgoing, auth references) that nothing
  consumes yet.
- Architecture contracts already documented: documentation/specification.md
  (async everything, UI never blocks), documentation/persistence.md §5
  (batch-per-transaction sync feeding a future envelope cache), and the
  crate-boundary rule that `nitidus-mail` stays UI-framework-free and testable
  with plain `#[tokio::test]`.

## 2. Proposal

### `nitidus-mail` (the domain crate grows its real skeleton)

- **`types.rs`** — newtypes and value types shared across the boundary:
  `AccountId`, `FolderId`, `EnvelopeId`, `JobId` (monotonic u64),
  `FolderMeta { id, name, unread, total }`, `EnvelopeSummary` (compact: id,
  subject, from display+addr, date epoch, flags bitfield),
  `ConnectionState { Disconnected, Connecting, Connected, Failed }`.
- **`error.rs`** — `MailError` via thiserror (backend, cancelled, channel-closed
  variants); errors are data that cross the channel, not panics.
- **`backend.rs`** — the `MailBackend` trait, async methods, scaffold surface:
  `list_folders()`, `scan_envelopes(folder, batch_tx)` (streams
  `Vec<EnvelopeSummary>` batches through a sender so backpressure applies inside
  the backend), `fetch_message(folder, id) -> Vec<u8>` (raw RFC 822),
  `set_flags(folder, id, flags)`. Backends are **generic, not boxed**: each
  actor is spawned monomorphized over its concrete backend
  (`spawn_account_actor<B: MailBackend>`), so no `async_trait`/dyn machinery —
  the backend for an account is chosen once at startup from config.
- **`command.rs` / `event.rs`** — the channel vocabulary:
  `MailCommand { ListFolders, SyncEnvelopes { folder, job }, FetchMessage { folder, id, job }, SetFlags { … }, Cancel(JobId), Shutdown }`;
  `MailEvent { Connection { account, state }, Folders { account, folders }, EnvelopeBatch { account, folder, job, batch, done }, Message { account, id, raw }, JobFailed { account, job, error } }`.
- **`actor.rs`** — one long-lived task per account owning its backend: `select!`
  over the account's command receiver and per-job `CancellationToken`s (held in
  a job table, removed on completion; `Cancel` triggers the token). Streamed
  batches go to the event channel with `send_async` — a slow UI applies
  backpressure naturally.
- **`engine.rs`** — `MailEngine`: owns the tokio runtime (2 workers, named
  threads), the bounded channels (commands 256/account, events 1024 shared), a
  `JobId` allocator, and the account registry. `add_account(id, backend)` spawns
  the actor; `send(account, command)`, `try_recv_event()` (non-blocking, for the
  drain), `next_job()`. `Drop` sends `Shutdown` to actors and drops the runtime.
- **`mock.rs`** (feature `mock`, enabled for tests) — a scripted in-memory
  backend: configurable folders/envelopes, artificial batch delays, failure
  injection — proves the actor/cancellation/backpressure machinery now and
  serves future UI tests and an offline demo mode.

### `crates/nitidus` (bin) — `engine.rs` plugin

- `EngineResource` wrapping `nitidus_mail::MailEngine` (Resource is a bin-side
  newtype — the domain crate stays bevy-free).
- `EngineStatus` resource: per-account `ConnectionState` map, surfaced in the
  statusline left segment next to the tab name (e.g. `mail ⋅ 0/0`) — minimal but
  real, so 1b.6 lights it up without UI work.
- `PreUpdate` drain system: `while let Ok(event) = try_recv_event()` capped at
  64 events per frame; scaffold routing: `Connection` → `EngineStatus`,
  everything else → tracing at debug (the real `MailStore` arrives with 1b.7).
- Accounts: none are registered at startup yet (no real backend exists until
  1b.6) — the app boots with an empty engine; the full
  command→actor→event→resource loop is proven by headless tests using the mock
  backend.

### Testing strategy

`nitidus-mail` gets its first real tests, plain `#[tokio::test]` (the no-bevy
invariant pays off immediately): actor lifecycle (spawn/shutdown), folder
listing via mock, envelope streaming in batches, **cancellation mid-stream**
(long scripted scan cancelled after the first batch; no further batches arrive),
failure injection → `JobFailed`, backpressure (bounded event channel; slow
consumer never loses events), engine drop shuts actors down. Bin-side headless
ECS test: engine with a mock account drains `Connection` events into
`EngineStatus` across updates. pty run proves the app still boots and quits
cleanly with the engine resource live.

Out of scope: any real backend (1b.6 Maildir, 1b.12 IMAP), the envelope
cache/`MailStore` (1b.7), account auto-registration from config (activates in
1b.6 when a constructible backend exists), IDLE/watch (backend-specific),
retry/reconnect policy (1b.12), send pipeline (1c.15).

## 3. Discussion

### 3.1 R1 Questions

1. **Generic actors over boxed trait objects**: backends monomorphize into their
   actor (`spawn_account_actor<B: MailBackend>`), avoiding
   `async_trait`/`Box<dyn>` — the account's backend type is known at startup
   from config. Costs: a small dispatch `match` in the bin when constructing
   accounts (1b.6+). Confirm?
2. **Scaffold trait surface**: `list_folders`, `scan_envelopes` (batched),
   `fetch_message`, `set_flags` — right minimal set? (`watch`/IDLE and folder
   ops arrive with the backends that implement them.)
3. **Mock backend placement**: permanent `mock` cargo feature on nitidus-mail
   (usable by future UI tests and an offline demo mode) — or test-only
   `#[cfg(test)]` module (invisible to the bin crate)? Proposal: permanent
   feature.
4. **Drain destination now**: `Connection` events → `EngineStatus` + statusline
   display; all data events → debug logs until 1b.7's MailStore. Acceptable, or
   would you rather the scaffold keep even status off-screen?
5. **Channel bounds / drain budget**: commands 256 per account, events 1024
   shared, drain ≤64 events/frame — accept defaults or tune?
6. **Runtime sizing**: fixed 2 worker threads for the engine runtime (per the
   architecture plan), or scale with account count later?

### 3.2 R1 Answers

1. confirm
2. looks good for now
3. confirm, permanent feature
4. acceptable
5. accept
6. scale

## 4. Plan

Each phase leaves the workspace compiling with clippy and tests green.

### Phase 1 — Domain types (`nitidus-mail`)

1. Dependencies: tokio (workspace + `time`), tokio-util, flume, thiserror,
   tracing. `[features] mock = []`; self dev-dependency enabling `mock` +
   tokio `macros` so `cargo test --workspace` runs the engine tests.
2. `types.rs` — `AccountId`/`FolderId` (cheap-clone `Arc<str>` newtypes),
   `EnvelopeId`, `JobId(u64)`, `Flags` bitfield (seen/answered/flagged/
   deleted/draft), `FolderMeta`, `EnvelopeSummary`, `ConnectionState`.
3. `error.rs` — `MailError` (thiserror): `Backend(String)`, `Cancelled`,
   `ChannelClosed`, `UnknownAccount`, `Runtime(io::Error)`.
4. `command.rs`/`event.rs` — vocabulary per §2, all `Debug + Clone`.
5. `backend.rs` — `MailBackend` trait with RPITIT
   (`-> impl Future<…> + Send`) methods; contract documented: a scan
   whose batch sender is disconnected stops and returns `Cancelled`.
6. Placeholder `crate_version()` and its smoke test retire.

### Phase 2 — Actor, engine, mock (`nitidus-mail`)

7. `actor.rs` — per-account task: command loop; `SyncEnvelopes` runs the
   backend scan through an intermediate bounded channel while
   `select!`ing over scan progress, batch forwarding (adding
   account/job context), the job's `CancellationToken`, and incoming
   commands (`Cancel` matching the job triggers the token; `Shutdown`
   cancels and exits; other commands defer to a queue processed after
   the scan). `Connection` events on start/stop.
8. `engine.rs` — `MailEngine::new(account_hint)` (workers =
   `(hint + 1).clamp(2, 4)`, named threads, R1.6), bounded channels
   (commands 256/account, events 1024 shared), `add_account<B>`,
   `send`, `try_recv_event`, `next_job`; `Drop` closes command channels
   (actors exit) and drops the runtime.
9. `mock.rs` (feature `mock`) — scripted backend: builder for
   folders/envelopes, generated envelope helper, `batch_size`,
   `batch_delay`, `fail_scan` injection; honors the disconnect→
   `Cancelled` contract.
10. `tests/engine.rs` (integration, `#[tokio::test]`): folder listing;
    batched envelope streaming with `done` terminal event;
    **cancellation mid-stream** (no batches after `Cancel`); failure
    injection → `JobFailed`; slow-consumer backpressure without loss;
    unknown account send errors; engine drop terminates actors.

### Phase 3 — Bin wiring

11. `engine.rs` (bin) — `EngineResource` newtype, `EngineStatus`
    (account → `ConnectionState`), `EnginePlugin` with the `PreUpdate`
    drain (≤64 events/frame; `Connection` → status, data events →
    debug log until 1b.7).
12. `run()` constructs `MailEngine::new(config.accounts.len())` before
    `build_app` (fallible, exits like config errors); `build_app`
    gains the engine parameter and adds `EnginePlugin`. No accounts
    registered yet (1b.6).
13. Statusline left segment appends `⋅ n/m` (connected/total) when
    `EngineStatus` is non-empty; refresh gating extended.
14. Bin headless test: engine + mock account (dev-dep feature) drains
    `Connection` events into `EngineStatus` across updates.

### Phase 4 — Verification

15. fmt/clippy/full suite; `cargo tree -p nitidus-mail` confirms no
    bevy in the dependency tree (the documented invariant, now
    mechanically checked and recorded in §5); pty run boots and quits
    cleanly with the engine live. Record in §5, commit per
    contributing.md.

## 5. Verification

All run 2026-07-24 on rustc/cargo 1.93.1:

- `cargo fmt --check` clean; `cargo clippy --workspace --tests` zero
  warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **80 passed, 0 failed**:
  nitidus-mail 2 unit + **7 integration** (connection lifecycle, folder
  listing, batched streaming with terminal event, cancellation
  mid-stream, failure injection → JobFailed, slow-consumer no-loss,
  unknown-account error); bin 55 (incl. the headless drain test reaching
  `1/1` connected via a mock account); ui-kit 15; contacts 1.
- **No-bevy invariant mechanically checked**:
  `cargo tree -p nitidus-mail -e normal` contains zero bevy crates.
- pty run (80×24): app boots with the engine resource live and `q`
  quits cleanly, exit 0.

## 6. Implementation Report

Implemented per §4. Two bugs found by the tests, one packaging gotcha:

- **Completed-scan batch loss** (caught by the batch-count test): the
  scan future can complete while batches still sit in the local bounded
  buffer — the actor dropped up to `SCAN_LOCAL_BUFFER + 1` batches. On
  scan completion the actor now drains the local channel (the sender
  drops when the future completes, so the drain terminates) before
  emitting the `done` event.
- **Borrowed-folder move**: the scan future borrows the folder id while
  the terminal event needs to move it — resolved with a clone scoped to
  the scan (`scan_folder`), keeping the original free.
- **Feature-unification mask**: `tokio::select!` needs tokio's `macros`
  feature in *normal* dependencies; tests passed without it because the
  dev-dependency self-reference unified features, while a plain
  `cargo build` failed. Caught by the phase-4 build; `macros` moved to
  the runtime dependency set. Lesson recorded: verify `cargo build` in
  isolation, not just `cargo test`.

Notes:

- Actor concurrency model: commands process sequentially; during a
  streaming scan the actor keeps receiving so `Cancel`/`Shutdown`
  interrupt mid-stream while other commands defer to a queue processed
  afterwards — one backend, no locks, no lost commands.
- Cancellation mechanics: the job's `CancellationToken` trips a select
  arm that drops the batch receiver; the backend contract (documented
  on the trait) turns the resulting send failure into
  `MailError::Cancelled`.
- `MailError` implements `Clone` manually (io::Error isn't Clone); the
  `Runtime` variant degrades to `Backend(String)` when cloned —
  acceptable for an error that only occurs before any event flows.
- Statusline shows `mail ⋅ n/m` only once accounts exist, so today's UI
  is unchanged; 1b.6 lights it up by registering the first real
  account.
- Follow-ups: account auto-registration from config activates in 1b.6;
  data events (Folders/EnvelopeBatch/Message) drain to debug logs until
  1b.7's MailStore; per-job progress reporting and reconnect policy are
  backend-item concerns.

## 7. Testing and Cleanup
