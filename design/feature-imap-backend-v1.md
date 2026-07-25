# feature - IMAP Backend - v1

Roadmap item 1b.12, the last of phase 1b. A remote `MailBackend` speaking IMAP
over TLS: password auth, folder listing with unread counts, streaming envelope
sync with session-scoped incremental updates (CONDSTORE), message fetch, flag
writes, folder create/delete/rename, IDLE push on INBOX, and connection status
in the statusline. With this, both Gmail test accounts work directly — mbsync
becomes optional.

## 1. Current Design

Everything above the backend trait is already backend-agnostic:

- `MailBackend` (RPITIT, one actor per account) has seven methods:
  `list_folders`, `scan_envelopes` (streaming batches through a flume sender;
  must return `Cancelled` when the receiver drops), `fetch_message`, `set_flags`
  (absolute flag set), and the three folder ops from 1b.13. Two implementations
  exist: maildir and mock.
- The store and SQLite cache reconcile scans with job-stamped upserts and
  **prune-on-done**: a scan that completes must have presented the full folder
  contents, or unseen rows are deleted. There are **no persisted sync cursors**
  (no UIDVALIDITY/MODSEQ columns).
- Maildir change-watching is an engine-level task (`watch_maildir`) emitting
  `FolderChanged` → the app re-syncs tracked folders and refreshes the folder
  list. IDLE needs the same shape: an actor's `&mut backend` cannot host a
  long-running wait.
- `ConnectionState::{Disconnected, Connecting, Connected, Failed}` events
  already drive the statusline `2/2` summary.
- Config is ready: `Backend::Imap { host, port (993), encryption }`,
  `Encryption::{Tls, Starttls, None}`, and
  `Auth::{Keyring (default), PasswordCmd, Oauth2}` all parse today;
  `bootstrap::register_accounts` surfaces a startup notice for unsupported
  backends.
- Library ground truth (`rust-libraries.md` §2): **async-imap 0.11**
  (chatmail/Delta Chat) is the verified recommendation — tokio,
  bring-your-own-TLS, IDLE with the 29-minute re-issue fixed, raw-command escape
  hatch, X-GM parsing for Phase 3. The roadmap/spec text still names io-imap
  0.2, but §1 of the same doc supersedes that: io-imap is weeks-old 0.x and the
  direct-protocol path was chosen with the trait boundary as the future
  migration seam.

## 2. Proposal

New module `crates/nitidus-mail/src/imap/` implementing `MailBackend` over
**async-imap 0.11 + tokio-rustls** (roots from `rustls-native-certs`).

### 2.1 Connection and auth

- `ImapConfig { host, port, encryption, auth }` built from the account config by
  the app; `nitidus-mail` stays free of config-file types.
- Connect: TCP → TLS (or STARTTLS; `Encryption::None` allowed but logged with a
  warning — it exists for the in-process test server) → LOGIN.
- Auth v1 is password-only: `Auth::PasswordCmd` runs the command and takes the
  first stdout line (both Gmail app passwords already live in files, so
  `password_cmd = "cat ~/.config/mbsync/kenianbei-pass"` works unchanged).
  `Keyring`/`Oauth2` accounts register with a startup notice ("auth lands with
  1d") exactly as IMAP itself does today.
- A `session` wrapper owns reconnection: any command hitting a broken connection
  emits `Connecting`, redials with capped exponential backoff, and replays once;
  repeated failure emits `Failed` and surfaces the error through the normal
  `JobFailed` path.

### 2.2 Folder listing

`LIST "" "*"` + per-folder `STATUS (MESSAGES UNSEEN)`:

- `FolderMeta.id` = the raw IMAP mailbox name; `name` = the same with the
  server's hierarchy delimiter replaced by `/` — the exact display shape the
  sidebar tree already splits. Modified-UTF-7 mailbox names are decoded for
  display (`utf7-imap` crate).
- `\Noselect` mailboxes (Gmail's `[Gmail]` container) are skipped — the sidebar
  synthesizes parents from paths anyway.
- INBOX is reported first by contract (matches maildir discovery).

### 2.3 Envelope sync

`scan_envelopes` keeps the store's prune-on-done contract by always streaming
the **full folder** — incrementality is an implementation detail behind it:

- The backend keeps a per-folder in-memory map
  `{ uidvalidity, highest_modseq, envelopes: BTreeMap<Uid, …> }`.
- First scan of a folder in a session: `SELECT` (with `CONDSTORE`), then
  `UID FETCH 1:* (UID FLAGS INTERNALDATE ENVELOPE BODY.PEEK[HEADER.FIELDS (REFERENCES IN-REPLY-TO)])`
  in windows, streamed as batches of 500 (matching the maildir batch size).
- Subsequent scans: `UID FETCH 1:* (FLAGS) (CHANGEDSINCE <modseq>)` for flag
  changes + a fetch of UIDs above the last seen for new mail + `SEARCH`
  reconciliation for expunges (or QRESYNC `VANISHED` where advertised); the
  merged map streams out in full, so the store and cache reconcile identically
  to a maildir scan.
- `UIDVALIDITY` mismatch discards the map and refetches from scratch;
  prune-on-done cleans the stale rows.
- Cursors are **session-scoped** in v1. Cold start still shows the cached
  envelopes instantly (warm start is untouched); the first scan re-fetches
  envelope headers from the server. Persisting UIDVALIDITY/MODSEQ into a schema
  v3 is deferred until the first-scan cost annoys in practice — the trait gains
  no cursor parameters today.
- `EnvelopeId` = the UID as a decimal string. Message-ID and References come
  from the header-fields fetch, feeding threading unchanged.

### 2.4 Message fetch, flags, folder ops

- `fetch_message`: `UID FETCH <uid> (BODY.PEEK[])` — mark-read stays the app's
  optimistic-flag decision, never a fetch side effect.
- `set_flags`: `UID STORE <uid> FLAGS (…)` — absolute replace, matching the
  trait contract; the Flags bitset maps to
  `\Seen \Answered \Flagged \Draft \Deleted`.
- Folder ops: `CREATE` (display path re-encoded with the server delimiter),
  `RENAME` (children follow per RFC 3501), `DELETE` guarded by a
  `STATUS (MESSAGES)` emptiness check first — the client-side refusal contract
  from 1b.13 holds uniformly across backends.

### 2.5 IDLE

Engine-level task per account, `watch_imap`, mirroring `watch_maildir`:

- Its own connection (an idling session can run nothing else), `SELECT INBOX`,
  `IDLE`; any untagged EXISTS/EXPUNGE/FETCH wakes it to emit
  `FolderChanged { INBOX }` — the existing handler re-syncs and refreshes the
  folder list.
- Re-issues IDLE every 25 minutes; reconnects with the same backoff policy; a
  dead IDLE task never affects the command connection.
- Non-INBOX freshness: the sidebar's folder switch currently syncs only
  never-tracked folders. This item changes `open_folder` to always
  `request_sync` the target — cancel-supersede makes it safe, maildir just
  re-scans cheaply, and IMAP revisits become incremental CHANGEDSINCE fetches
  (cheap by §2.3).

### 2.6 Wiring and testing

- `bootstrap::register_accounts` gains the `Backend::Imap` arm: build
  `ImapConfig`, `engine.add_account`, `engine.watch_imap`, ListFolders + eager
  INBOX sync — identical shape to maildir registration. The cache and warm start
  need no changes.
- Deterministic tests use an **in-process scripted IMAP server** (a tokio
  `TcpListener` helper in `tests/common/` speaking canned
  greeting/LOGIN/LIST/SELECT/FETCH/STORE exchanges over plaintext with
  `Encryption::None`) — covering connect/auth failure, folder listing, full +
  incremental scan flows, flag writes, and folder ops, without network. Live
  verification against both Gmail accounts happens in the pty smoke.
- New workspace deps: `async-imap` (runtime-tokio), `tokio-rustls`,
  `rustls-native-certs`, `utf7-imap`.

## 3. Discussion

### 3.1 R1 Questions

1. **async-imap over io-imap.** The roadmap text names io-imap, but
   `rust-libraries.md`'s verified assessment recommends async-imap 0.11 (io-imap
   is weeks-old 0.x; re-evaluate in 6–12 months behind the trait seam). Proceed
   with async-imap?
2. **Session-scoped cursors.** First scan per session re-fetches all envelope
   headers (~2 s for a few thousand messages; the UI shows warm-start cache
   instantly meanwhile). Persisted UIDVALIDITY/MODSEQ (cache schema v3 + a trait
   cursor parameter) is deferred. Acceptable for v1?
3. **Password auth only.** `password_cmd` is the one working auth until 1d
   (`keyring`/`oauth2` register a notice). Your existing app-password files slot
   in directly. Confirm?
4. **Folder-switch resync.** Switching folders always re-requests a sync (not
   just first view) so IMAP folders are fresh on entry; maildir re-scans are
   cheap and watcher-covered anyway. Any objection?
5. **IDLE scope.** One IDLE connection per account, INBOX only; other folders
   refresh on view (Q4) and via the folder-list refresh. A per-viewed-folder
   IDLE is a follow-up if INBOX-only feels stale. OK?
6. **Scripted-server testing.** Deterministic integration tests against an
   in-process fake IMAP server (plaintext, canned responses), plus live Gmail
   pty smoke — no test-time dependency on dovecot/greenmail. Sound right?

### 3.1 R1 Answers

1. no, use io-imap and any other libraries from himalaya. please add a note to
   ignore recommendations from and files in documentation except
   specification.md and roadmap.md.
2. yes
3. yes, but let's store plaintext passwords in our config directory, or make it
   configurable. We should not be using other app .config dirs.
4. agreed
5. ok
6. agreed

### 3.3 R2 Notes

1. **io-imap it is.** `rust-libraries.md` now carries a banner: only
   `specification.md` and `roadmap.md` are authoritative; other
   `documentation/` files are historical research. Pimalaya `io-*`
   crates are preferred throughout (io-smtp lands with 1c, io-oauth with
   1d).
2. **io-imap 0.2 architecture** (verified from source): sans-IO
   *coroutines* — every command is a resumable state machine yielding
   `WantsRead`/`WantsWrite`; the caller owns the socket and pumps bytes
   (its `tokio_coroutine` example shows the exact tokio + tokio-rustls
   pump this design adopts). Coverage is complete for our needs:
   greeting, LOGIN/SASL, LIST, STATUS, SELECT/EXAMINE (CONDSTORE/QRESYNC
   fields on `ImapMailboxSelectData`), FETCH (+`CHANGEDSINCE` modifier,
   streaming variant), STORE (silent), CREATE/RENAME/DELETE, SEARCH,
   NOOP, LOGOUT — and `ImapMailboxWatch`, which packages the entire
   IDLE + QRESYNC-reselect watch loop on a dedicated connection with an
   `AtomicBool` shutdown, replacing §2.5's hand-rolled IDLE plumbing.
   imap-codec/imap-types are re-exported. This *replaces* §2's
   async-imap wiring; §2.2–§2.4 protocol behavior is unchanged.
3. **Passwords in our config directory.** New auth variant
   `password_file` with a configurable path (`~` expansion; a relative
   path resolves against the nitidus config dir). Smoke setup moves both
   Gmail app passwords to `~/.config/nitidus/` 0600 files and the
   mbsync-file references go away. `password_cmd` remains for external
   secret managers.
4. Dependencies become: `io-imap` (client feature off — we pump the
   coroutines ourselves), `tokio-rustls` + `rustls-platform-verifier`
   (the pattern io-imap's own example uses), `utf7-imap` for mailbox
   name display decoding.

## 4. Plan

Each phase leaves the workspace compiling, clippy-clean, and tests green.

**Phase 1 — auth config + docs note.** `Auth::PasswordFile { path }` in
the account config (parse tests; `~`/relative resolution helper), the
`rust-libraries.md` authority banner (already applied with this doc
round). `resolve_password` helper: file → first line; command → first
stdout line; keyring/oauth2 → descriptive error for the startup notice.

**Phase 2 — connection core.** `crates/nitidus-mail/src/imap/` with:
`stream.rs` (an `ImapStream` enum over plain TCP / TLS with a unified
`AsyncRead + AsyncWrite`), `pump.rs` (generic tokio pump driving any
`ImapCoroutine` over the stream + connection-wide `Fragmentizer`, plus
the richer-yield pump for watch), `connect.rs` (TCP → TLS or STARTTLS →
greeting → LOGIN; `ImapConfig { host, port, encryption, password }`),
`session.rs` (owns stream + fragmentizer; `run(coroutine)` entry;
reconnect-once with capped backoff on IO failure, `Connecting`/`Failed`
connection events through a channel handed in at construction). The
scripted-server test harness (`tests/common/imap_server.rs`: tokio
`TcpListener`, canned request→response script, plaintext) proves
greeting/LOGIN and the reconnect path.

**Phase 3 — read path.** `folders.rs`: LIST + per-mailbox STATUS
(MESSAGES UNSEEN) → `FolderMeta` (delimiter→`/`, utf7 display decode,
`\Noselect` skipped, INBOX first). `envelopes.rs`: windowed
`UID FETCH … (UID FLAGS INTERNALDATE ENVELOPE BODY.PEEK[HEADER.FIELDS
(REFERENCES IN-REPLY-TO)])` → `EnvelopeSummary` (UID-string ids, flag
mapping, references parsing reused from the maildir message module).
`backend.rs`: `MailBackend` impl for `list_folders`, `scan_envelopes`
(full fetch, 500-batches, `Cancelled` on receiver drop),
`fetch_message` (BODY.PEEK[]), `set_flags` (UID STORE silent).
Bootstrap gains the `Backend::Imap` arm (ListFolders + eager INBOX,
notice for keyring/oauth2). Fake-server tests for listing, scan,
fetch, flags.

**Phase 4 — incremental sync + folder ops.** Per-folder session state
`{ uid_validity, highest_mod_seq, envelopes: BTreeMap<u32, EnvelopeSummary> }`;
re-scans SELECT (CONDSTORE), fetch `CHANGEDSINCE` flag deltas + new
UIDs, drop vanished (QRESYNC where advertised, else UID SEARCH ALL
reconciliation), stream the merged full map; UIDVALIDITY mismatch →
full refetch. Folder CRUD (`CREATE`/`RENAME`/`DELETE` with the STATUS
emptiness guard, delimiter re-encoding). Sidebar `open_folder` switches
from sync-if-untracked to always-request-sync. Fake-server tests for
the incremental flows and CRUD.

**Phase 5 — IDLE watch + wiring polish.** `engine.watch_imap(account,
config)`: dedicated connection running `ImapMailboxWatch` on INBOX;
watch events → `FolderChanged { INBOX }`; reconnect with backoff;
shutdown flag tied to engine shutdown. Connection status events from
the command session surface in the statusline. Fake-server watch test
(scripted IDLE wake).

**Phase 6 — live smoke + docs.** Move both app passwords to
`~/.config/nitidus/`, switch `config.toml` to
`backend = { imap = { host = "imap.gmail.com" } }` +
`auth = { password_file = { path = "…" } }` for kenianbei (norman
account left on maildir to exercise mixed backends), pty smoke: cold
start folder list, INBOX sync, threading, pager fetch, flag write,
folder switch, IDLE-driven refresh if observable. Record results;
update the gmail-maildir memory.

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **218 passed, 0 failed**
  (was 204 pre-feature: +4 secrets tests, +3 imap folder-name unit
  tests, +7 scripted-server integration tests; the bootstrap notices
  test grew a password-file registration case).
- Scripted-server coverage: folder listing with `\Noselect` skip and
  STATUS counts, full scan streaming, incremental re-scan (CHANGEDSINCE
  flag delta + new-UID window + UID SEARCH reconciliation), UIDVALIDITY
  bump forcing a refetch, message fetch + flag store, folder ops with
  the empty-delete guard, auth failure surfacing, and reconnect-once
  after a dropped connection.
- Live smoke against `imap.gmail.com` (kenianbei on IMAP, norman on
  maildir — mixed backends): cold start listed all folders over IMAP,
  the 604-message INBOX full-scanned and the cache reconciled from
  maildir-name ids to UID ids (prune-on-done verified in SQLite),
  statusline `2/2`, `[Gmail] (19)` collapsed badge from IMAP STATUS
  counts. `Tab j Enter` lazily synced Condo over IMAP; `Enter` opened a
  real message via `UID FETCH BODY.PEEK[]`. IDLE fallback connected and
  held; clean exits throughout.

## 6. Implementation Report

Implemented per plan on io-imap 0.2, with these findings:

- **Engine runtime bug (pre-existing):** the mail runtime was built
  without `enable_io`, so the first real socket use panicked
  (`TcpStream` requires the IO driver). One-line fix in
  `MailEngine::new`; every earlier backend was filesystem-only, which
  is why it never surfaced.
- **Gmail has CONDSTORE but not QRESYNC**, so io-imap's
  `ImapMailboxWatch` (QRESYNC-required) refused. The watch task now
  dispatches: QRESYNC servers get `ImapMailboxWatch`, everyone else
  plain RFC 2177 `ImapIdle` — both reduced to `FolderChanged { INBOX }`
  since the normal re-scan is already incremental. A 20-minute read
  timeout re-establishes silently dead IDLE connections, and a
  healthy-run check resets the reconnect backoff.
- **utf7-imap panics on malformed input**; mailbox names come from the
  network, so display decoding wraps it in `catch_unwind` with a
  raw-name fallback (unit-tested).
- **Shared envelope parsing:** `summarize_headers` extracted into
  `nitidus-mail/src/envelope.rs`; maildir and IMAP now decode
  subjects/addresses/references through the same mail-parser path
  (IMAP fetches `BODY.PEEK[HEADER.FIELDS (…)]` instead of ENVELOPE
  precisely so the semantics match).
- Session `run` takes a coroutine *factory* so the reconnect-once retry
  can rebuild the command; the selected-mailbox marker dies with the
  connection, making retried UID commands re-select automatically.
- Connection status stays actor-level (Connected on spawn, Disconnected
  on shutdown) — the session does not yet emit `Connecting`/`Failed`
  transitions because backends have no event-channel handle; deferred
  to the 1d/IMAP-polish rounds along with persisted cursors (schema
  v3), `Cancel` responsiveness *during* a fetch window (cancellation
  currently lands between windows), and decoded RFC 2047 header values
  in the pager's header display (visible with real Gmail mail; a pager
  follow-up, not IMAP).
- Live config now: kenianbei on `imap.gmail.com` with
  `password_file = "kenianbei-password"` (0600, in the nitidus config
  dir); norman.kerr.dev intentionally left on the mbsync maildir to
  keep exercising mixed backends.

## 7. Testing and Cleanup
