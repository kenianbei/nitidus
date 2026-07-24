# feature - maildir backend - v1

The first real `MailBackend` implementation: a hand-rolled Maildir backend
(local filesystem, no auth, no network) with folder discovery, batched envelope
scanning, flag updates via the `:2,` rename protocol, message fetch, and
notify-based change detection. This is roadmap item 1b.6 — after it lands,
accounts from config register with the engine at startup, the statusline
connection counter goes live, and real mail flows through the machinery built in
1a.5 (visible in logs until the envelope cache/index land in 1b.7–8).

## 1. Current Design

- **The consuming machinery exists and is tested** (1a.5, merged): `MailBackend`
  trait (`list_folders`, `scan_envelopes` streaming batches through a sender
  with the disconnect→`Cancelled` contract, `fetch_message`, `set_flags`),
  per-account actors with interruptible scans, `MailEngine::add_account`, and
  the bin-side drain + `EngineStatus` statusline counter. Only `MockBackend`
  implements the trait today; no accounts register at startup.
- **Config parses maildir accounts** (1a.3):
  `Backend::Maildir(MaildirBackend { path: PathBuf })` — with a noted follow-up
  that `~` in paths is not expanded.
- **No maildir or MIME code exists.** documentation/rust-libraries.md decided:
  hand-roll Maildir (~500 LOC; the spec — unique name in `tmp/`, link into
  `new/`, rename into `cur/` with `:2,` flags — is tiny, and owning it controls
  flag semantics), `mail-parser` 0.11 for MIME (Apache/MIT, zero-copy,
  encoded-words), `notify` 8 + debouncing for change watching, with the gotcha
  recorded: watch each folder's `new/` and `cur/` **non-recursively** (inotify
  allocates per directory) and debounce because delivery emits tmp-write +
  rename pairs. None of these are workspace dependencies yet.
- **Events vocabulary** (1a.5) has no "folder changed externally" variant; the
  actor model runs one `&mut backend` at a time, so a long-running watch cannot
  live inside a backend method — watching needs an engine-level home.
- Maildir layout reality (mbsync/offlineimap-compatible): the account root is
  itself a maildir (INBOX with `cur/new/tmp`); subfolders are either Maildir++
  (`.Archive.2024/` dot-encoded) or plain subdirectories containing
  `cur/new/tmp`.

## 2. Proposal

### `nitidus-mail` — `maildir/` module (new deps: mail-parser, notify)

- **`maildir/folders.rs`** — discovery: the root is `INBOX`; any child directory
  containing `cur` + `new` + `tmp` is a folder. Maildir++ dot-names decode to
  display paths (`.Archive.2024` → `Archive/2024`); plain directories keep their
  name. `FolderId` = the directory name (stable), `FolderMeta.name` = decoded
  display name; `unread` = count of files in `new/`, `total` = `new/ + cur/`.
- **`maildir/message.rs`** — file-level operations:
  - Envelope parse: read up to a 64 KiB header window (or full file if smaller),
    `mail-parser` extracts subject/from/date; date falls back to file mtime;
    flags decode from the `:2,` suffix (S=seen, R=answered, F=flagged,
    T=deleted, D=draft) with files in `new/` implicitly unseen.
  - `EnvelopeId` = the maildir unique name (everything before `:2,`), stable
    across flag renames; lookup scans `cur/` + `new/` for the prefix.
  - `set_flags` = rename to the new `:2,` suffix, moving `new/ → cur/` (a
    message that gains flags is no longer "new"); `fetch_message` = full file
    read.
- **`maildir/backend.rs`** — `MaildirBackend::new(root: PathBuf)` implementing
  `MailBackend`: `scan_envelopes` walks `new/` then `cur/`, batching 500
  envelopes per send (honoring the cancellation contract); blocking filesystem
  work runs via `tokio::task::spawn_blocking` per batch chunk so the actor's
  runtime threads never block on IO.
- **`watch.rs`** — engine-level watching (not a backend method, per the
  &mut-actor constraint): `MailEngine::watch_maildir(account, root)` spawns a
  task owning a `notify` recommended-watcher over each folder's `new/` and
  `cur/` (non-recursive), coalescing raw events per-folder with a 500 ms quiet
  window, emitting a new `MailEvent::FolderChanged { account, folder }` into the
  existing events channel. Consumers (1b.7+) react by issuing `SyncEnvelopes`;
  until then the drain logs it.

### `crates/nitidus` (bin) — account registration

- `engine::register_accounts(engine, &config)` called from `run()` after engine
  construction: `Backend::Maildir` accounts construct `MaildirBackend` (leading
  `~/` expanded bin-side via the home dir) and register + start watching;
  accounts with no backend or an IMAP backend log a startup status message
  ("imap not yet supported") and surface once in the statusline; registration
  failures (missing path) are startup errors like config errors.
- On registration the bin sends `ListFolders` once per account — folders land in
  debug logs today, proving the pipeline end-to-end; envelope sync begins when
  the MailStore (1b.7) exists to receive it.
- Statusline `mail ⋅ n/m` lights up for real (first visible change).

### Testing strategy

Integration tests over `tempfile` maildirs built by a test fixture (real files,
real renames): folder discovery across layouts (root INBOX, Maildir++ dots,
plain subdirs, non-folder dirs ignored); envelope scan (flags from suffixes,
`new/` unseen, subject/from/date from real RFC 822 fixtures, mtime fallback,
batching across new+cur); `set_flags` rename semantics (suffix rewritten,
`new/ → cur/` move, id stable after rename); `fetch_message` round-trip; scan
cancellation (sender dropped mid-scan → `Cancelled`); watcher emits
`FolderChanged` for the touched folder within the debounce window (and only once
per burst). Bin-side: `register_accounts` with a tempdir maildir account reaches
`1/1` in `EngineStatus` and skips an IMAP account with a notice. pty run with
`NITIDUS_CONFIG_DIR` pointing at a maildir account config: statusline shows
`mail ⋅ 1/1`.

Out of scope: the envelope cache / `MailStore` and sync-on-change (1b.7), the
index UI (1b.8), IMAP (1b.12), Maildir _writing_ beyond flag renames (message
delivery/append arrives with the send pipeline, 1c.15), folder
create/delete/rename (1b.13).

## 3. Discussion

### 3.1 R1 Questions

1. **Watching in this item**: engine-level notify watcher emitting a new
   `MailEvent::FolderChanged` with 500 ms coalescing, as proposed — or split
   watching into its own follow-up item and keep 1b.6 to the read path? (Roadmap
   lists watching under 1b.6; the proposal keeps it, engine-level, because the
   actor's `&mut backend` model can't host a long-running watch.)
2. **Folder discovery rule**: any subdir with `cur/new/tmp` is a folder;
   Maildir++ dot-names decode to `A/B` display names; root = INBOX. Covers
   mbsync/offlineimap/dovecot layouts — confirm?
3. **Header-window parsing**: 64 KiB header window per message for the scan
   (full read only on fetch) — accept, or full-file reads for simplicity at the
   cost of scan speed on large messages?
4. **`~` expansion**: leading-`~/` expanded bin-side at backend construction
   (closing the 1a.3 follow-up) — confirm?
5. **Startup behavior**: register + watch + one `ListFolders` per account
   (pipeline proof in logs; statusline lights up), envelope sync deferred to
   1b.7 — or also fire an initial `SyncEnvelopes` whose batches go to debug
   logs?
6. **Unsupported accounts**: IMAP/backend-less accounts log + show a one-time
   statusline notice — or fail startup (strict) until IMAP exists?

### 3.2 R1 Answers

1. Can I get more explanation for this? What's the pro/cons?
2. confirm
3. accept
4. confirm
5. also fire intial sync
6. first option

Also, at what point will we be able to smoke-test with real mail data?

### 3.3 R2 Discussion

**Watching pros/cons (question 1).** For including now: maildir without
watching is stale the moment it's scanned (mbsync deliveries invisible
until manual refresh); the `FolderChanged` event surface gets settled so
1b.7 just consumes it; the watcher is engine-level and isolated (~100
lines), so its risk doesn't touch the read path; filesystem-watching
platform quirks (atomic-save rename storms, NFS/WSL silence, inotify
limits) surface now rather than mid-1b.7. For splitting: notify is an
FFI dependency tree whose quirks could eat unrelated debugging time;
nothing consumes the event until 1b.7 so deferral loses nothing; the
item is already 1b's largest. **Decision: include now** (user-confirmed,
recommended).

**Real-mail smoke testing (follow-up question).** Two stages: at the end
of *this* item, pointing config at a real maildir (e.g. an mbsync dir)
shows real `mail ⋅ 1/1` in the statusline and, with the R1.5 initial
sync, real folders and envelope batches in `RUST_LOG=debug` logs — real
data, log-visible. Mail **on screen** arrives after 1b.8 (1b.7 MailStore
+ 1b.8 index), two items out. An optional manual smoke test against a
real maildir is added to §5.

## 4. Plan

Each phase leaves the workspace compiling with clippy and tests green.

### Phase 1 — Dependencies + folder discovery

1. Workspace: add `mail-parser = "0.11"`, `notify = "8"`; nitidus-mail
   gains both.
2. `maildir/folders.rs`: `discover(root) -> Vec<FolderMeta>` — root is
   `INBOX` (validated to contain `cur/new/tmp`); child dirs with
   `cur+new+tmp` are folders; Maildir++ dot-names decode for display;
   `folder_dir(root, &FolderId)` maps id → path; unread/total from
   `new/`/`cur/` counts.

### Phase 2 — Messages + backend

3. `maildir/message.rs`: `parse_envelope(path, in_new)` — 64 KiB header
   window, mail-parser subject/from/date (mtime fallback), `:2,` flag
   suffix decode (D/F/R/S/T, ASCII-ordered), `EnvelopeId` = unique name
   before `:2,`; `find_message(dir, id)`, `rename_with_flags` (suffix
   rewrite + `new/ → cur/` move).
4. `maildir/backend.rs`: `MaildirBackend::new(root)` (validates
   layout), `MailBackend` impl — scan walks `new/` then `cur/`,
   parses per 500-file chunk inside `spawn_blocking`, honors the
   disconnect→`Cancelled` contract; fetch = full read; set_flags =
   rename.
5. Integration tests over tempfile maildir fixtures (per §2 testing
   strategy).

### Phase 3 — Watching

6. `MailEvent::FolderChanged { account, folder }` variant.
7. `watch.rs`: `MailEngine::watch_maildir(account, root)` — spawned
   task owns a notify recommended-watcher on each folder's `new/` and
   `cur/` (non-recursive), maps paths back to folders, coalesces with a
   500 ms quiet window (tokio timeout loop), emits one `FolderChanged`
   per changed folder per burst. New folders created after startup are
   not watched (recorded limitation; revisit with 1b.13 folder ops).
8. Watcher integration test: touch a delivery into `new/`, expect
   exactly one `FolderChanged` for that folder.

### Phase 4 — Bin registration

9. `engine::register_accounts(&mut MailEngine, &Config) ->
   anyhow::Result<Vec<String>>` (returned strings = startup notices):
   maildir accounts expand leading `~/` (etcetera home dir), construct
   the backend (missing/invalid root = startup error), register, start
   watching, send `ListFolders` + initial `SyncEnvelopes` for INBOX
   (R1.5; full multi-folder sync orchestration is 1b.7's job); IMAP or
   backend-less accounts push a notice.
10. `run()` calls it between engine construction and `build_app`;
    notices ride into the app as a `StartupNotices` resource surfaced
    once through `StatusMessage` at startup.
11. Bin tests: tempdir maildir account reaches `1/1`; IMAP account
    produces a notice without failing startup.

### Phase 5 — Verification

12. fmt/clippy/full suite; `cargo tree` no-bevy check still clean; pty
    run with a tempdir maildir config shows `mail ⋅ 1/1`; optional
    manual smoke test against a real maildir (user-provided path,
    `RUST_LOG=debug` shows real folders + envelope batches). Record in
    §5, commit per contributing.md.

## 5. Verification

All run 2026-07-24 on rustc/cargo 1.93.1:

- `cargo fmt --check` clean; `cargo clippy --workspace --tests` zero
  warnings; `cargo tree -p nitidus-mail -e normal` still contains zero
  bevy crates (mail-parser + notify added without violating the
  invariant).
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **94 passed, 0
  failed**: 6 new maildir integration tests (folder discovery across
  layouts, envelope scan with flags/headers/date, non-maildir root
  rejection, flag rename + new→cur move + fetch round-trip, dropped-
  receiver cancellation, watcher burst coalescing to one
  `FolderChanged`), 3 new bin registration tests (maildir account
  reaches `1/1`, IMAP account produces a notice not an error, missing
  path fails startup naming the account), plus all prior suites.
- pty run (80×24) with `NITIDUS_CONFIG_DIR` pointing at a generated
  maildir account: statusline renders `mail ⋅ 1/1`; `q` exits 0.
- Initial-sync proof (R1.5): `RUST_LOG=debug` log contains the real
  `Folders` event and `EnvelopeBatch` for INBOX including the fixture
  message subject — command → actor → backend → event → drain verified
  end-to-end with real files.
- Real-mailbox smoke test (optional step): ready — point
  `backend = { maildir = { path = "~/Mail/..." } }` at an mbsync
  maildir and check the statusline + debug logs; on-screen mail arrives
  with 1b.7–8.

## 6. Implementation Report

Implemented per §4 with these notes:

- The scan parses per-chunk inside `spawn_blocking` and skips
  unreadable messages with a warning rather than failing the whole scan
  — one corrupt file must not hide a mailbox.
- Watcher shape: notify's callback bridges into a flume channel; the
  async task coalesces with a 500 ms quiet window and maps paths back
  to folders (`<root>[/<dir>]/{new,cur}/…`). Recorded limitation:
  folders created after startup are unwatched until restart (revisit
  with folder ops, 1b.13).
- `EnginePlugin` now initializes `StatusMessage` itself — the notices
  system needs it, and plugin-order coupling to ShellPlugin was an
  avoidable trap (found by the headless registration test panicking on
  the missing resource).
- `~` expansion landed bin-side via `etcetera::home_dir()`, closing the
  1a.3 follow-up.
- Startup notices ride a `StartupNotices` resource surfaced once
  through the statusline (warning severity) — IMAP accounts degrade
  gracefully until 1b.12.
- Follow-ups: `FolderChanged` is drained to debug logs until 1b.7 wires
  resync; initial sync covers INBOX only (multi-folder orchestration is
  1b.7's job); `EnvelopeSummary` has no message-id/references yet —
  added when threading (1b.9) needs them.

## 7. Testing and Cleanup
