# Persistence — State Storage Design and Rust Best Practices

How nitidus persists the address book, drafts, caches, and application
state across runs; which storage form each kind of state gets and why; and
the Rust idioms that keep all of it crash-safe. Crate choices reference
[rust-libraries.md](rust-libraries.md).

## 1. Guiding principles

1. **Right store for the shape of the data.** Plain files for
   human-meaningful, low-volume records (contacts, drafts, config);
   SQLite for high-volume, queryable, transactional data (envelope cache,
   sync state, FTS); small serialized files for disposable UI state.
   Never one giant database holding everything, never thousands of tiny
   files where a query engine is needed.
2. **The mail server is the source of truth for mail; local files are the
   source of truth for contacts and config; everything else is a cache.**
   Any cache must be safely deletable: `rm -rf ~/.cache/nitidus` must
   never lose user data, only cost a re-sync.
3. **Crash-safe by construction**: atomic write-via-rename for files,
   WAL-mode transactions for SQLite, and recovery paths for anything
   in-flight (compose sessions).
4. **No secrets on disk in plaintext.** Tokens and passwords go to the OS
   keyring or a user-configured credential command; state files never
   contain credentials.

## 2. Directory layout (XDG)

Resolved via the `etcetera` crate (explicit XDG strategy; honors
`XDG_*_HOME` overrides):

```
~/.config/nitidus/            # user-authored, precious, backup-worthy
├── config.toml               # accounts (no secrets), UI, behavior
└── keys.toml                 # keybindings per context

~/.local/share/nitidus/       # user data, precious, backup-worthy
├── contacts/
│   └── <uid>.vcf             # one vCard per contact
├── drafts/
│   └── <compose-id>.eml      # RFC 822 draft messages
└── outbox/
    └── <job-id>.eml          # queued/failed sends (never lose mail)

~/.local/state/nitidus/       # per-machine state, losable but useful
├── ui.toml                   # tab layout, last folder, sort prefs
├── history/                  # command-line + search history
└── nitidus.log               # tracing output

~/.cache/nitidus/             # deletable at any time
├── mail.db                   # SQLite: envelopes, sync state, FTS5
├── bodies/                   # cached message bodies (content-addressed)
├── htmlshots/                # tier-3 rendered PNGs (blake3-keyed)
└── photos/                   # contact photo thumbnails
```

The config/data/state/cache split is the load-bearing decision: it tells
users (and backup tools) exactly what is precious. Windows/macOS get the
equivalent platform dirs from etcetera automatically.

## 3. Address book

**Format: one `.vcf` file per contact, UID as filename** (vCard 4.0 via
calcard), in `~/.local/share/nitidus/contacts/`.

Why files, not a database:

- Contacts are low-volume (hundreds to low thousands) — no query engine
  needed; nitidus holds them all in memory (as vcard_tui already does)
  and the `ContactIndex` autocomplete map is rebuilt on change.
- **This is the vdir layout** (vdirsyncer/pimsync convention): each file
  is one resource, which is exactly what CardDAV sync wants — the CardDAV
  ETag maps to a per-file sidecar entry, and sync becomes per-file
  compare-and-swap. Interop with khard/vdirsyncer comes free.
- Human-recoverable and diffable: a corrupted store is N-1 good files,
  not one bad binary blob; users can git-version the directory.

Write path (every mutation): serialize → write to
`contacts/.tmp.<uid>.vcf` → `fsync` the file → `rename` over the target →
`fsync` the directory. Rename within one filesystem is atomic on POSIX;
readers never observe a half-written vCard. (Use the `tempfile` crate's
`NamedTempFile::persist` in the same directory — same-filesystem rename
guaranteed.)

Sync metadata (CardDAV ETags, sync tokens, tombstones for deletions) lives
in `mail.db`, not in the `.vcf` files — deleting the cache forces a full
re-compare but loses nothing.

The frecency-ranked harvested-address store (compose autocomplete
suggestions, not real contacts) is cache-tier: a table in `mail.db`.

## 4. Drafts and outbound mail

**Format: RFC 822 `.eml` files** — the mail-native representation, built
with mail-builder, parseable by any tool.

- **Compose sessions**: from the first keystroke, the session's body file
  lives in `drafts/` (not tmpfs) with a small TOML sidecar carrying
  headers/attachment list/session phase. Crash → next start scans
  `drafts/`, finds sessions with no clean-shutdown marker, offers
  recovery (aerc's `:recover`, made automatic).
- **Postpone** (`:postpone`): the draft is APPENDed to the account's
  Drafts folder (server round-trip so it roams to other clients), and the
  local file is kept until the server copy is confirmed, then removed.
  Recall (`:recall`) fetches it back into a session.
- **Outbox queue**: `:send` moves the finalized message to `outbox/` first,
  then the send job streams it via SMTP. Success → delete (Gmail) or
  APPEND to Sent then delete (Outlook/IMAP — see the Sent-Items
  difference in [gmail.md](gmail.md)/[outlook.md](outlook.md)). Failure →
  the file stays, flagged in the UI for retry. **A send can fail, crash,
  or lose the network at any point without losing the message.** This
  queue is also what makes undo-send (delayed send) trivial: the job just
  waits N seconds before opening the connection, and `:undo` deletes the
  file.

## 5. Envelope cache and sync state (SQLite)

`~/.cache/nitidus/mail.db` via rusqlite (`bundled`), owned exclusively by
the mail runtime (UI reads go through `MailStore` in memory, never the DB).

- **Pragmas**: `journal_mode=WAL` (readers never block the writer, crash
  recovery built in), `synchronous=NORMAL` (safe with WAL, much faster),
  `foreign_keys=ON`, `busy_timeout` set. This pragma set is the
  industry-standard Rust/SQLite configuration.
- **Schema (sketch)**: `accounts`, `folders` (with `uidvalidity`,
  `uidnext`, `highestmodseq`, sync cursor), `envelopes` (UID, message-id
  hash, thread keys, from/subject/date, flags bitfield, gmail msgid/thrid
  where present), `labels`/`envelope_labels` (Gmail multi-label,
  categories later), `carddav_state`, `harvested_addrs`, plus an FTS5
  external-content table over subject/from (bodies optional, off by
  default).
- **Transactions batch per sync chunk** (one transaction per 500-envelope
  batch, matching the engine's `EnvelopeBatch` size) — never a
  transaction per row, never one giant transaction per folder.
- **UIDVALIDITY discipline**: if a folder's UIDVALIDITY changes, drop and
  re-sync that folder's rows — cache-tier data makes this a non-event.
- **Migrations**: `PRAGMA user_version` + an ordered list of idempotent
  migration functions run at open (the `rusqlite_migration` crate or ~30
  hand-rolled lines). Because mail.db is cache-tier, the escape hatch for
  a botched migration is "delete and re-sync" — which is why nothing
  precious is allowed to live there. **The version stamp still matters**:
  nitidus must refuse to open a *newer* schema than it knows (downgrade
  protection).
- Message bodies: content-addressed files under `bodies/` (blake3 of the
  raw message), with an LRU eviction sweep by atime/size cap — large
  blobs don't belong inside SQLite rows, and content addressing dedupes
  Gmail's one-message-many-folders problem for free.

## 6. UI / session state

`~/.local/state/nitidus/ui.toml`: open tabs and kinds, selected
folder/message per tab, sort modes, collapsed-thread sets (capped),
sidebar width, last-used theme. Serialized with serde + toml.

Rules:

- Written atomically (same tempfile+rename discipline) on clean exit and
  debounced (~5s) during use — a crash loses at most a few seconds of
  ephemera.
- **Parse failures are non-fatal by contract**: any error → log, rename
  the bad file to `ui.toml.broken`, start with defaults. UI state must
  never be able to brick startup.
- Deserialization uses `#[serde(default)]` throughout so fields can be
  added/removed across versions without migration machinery.

Command/search history: append-only line files with periodic truncation
(the shell-history model) — append is naturally crash-tolerant.

## 7. Secrets

- OAuth refresh tokens, IMAP passwords: **keyring crate** (Secret
  Service/keychain/Credential Manager), calls wrapped in
  `spawn_blocking`; or the user configures a credential command
  (`pass show …`) per account, which nitidus shells out to.
- `config.toml` stores only *references* (`password_cmd`,
  `auth = "oauth2"`), never secret material. Files that might contain
  anything sensitive (outbox, drafts) are created `0600`; the config dir
  is checked and warned about if group/world-readable (aerc's
  accounts.conf discipline).

## 8. Rust best-practice summary (the idioms, condensed)

1. **Atomic replace**: `tempfile::NamedTempFile` in the *target
   directory* → write → `as_file().sync_all()` → `persist(path)` →
   `File::open(dir)?.sync_all()`. Never truncate-and-rewrite in place.
2. **WAL + NORMAL + user_version** for every SQLite database; one writer
   thread (tokio-rusqlite / `spawn_blocking`), batched transactions.
3. **serde everywhere, with `#[serde(default)]` and
   `#[serde(deny_unknown_fields)]` chosen deliberately**: config wants
   strictness (typos should error loudly); state files want leniency
   (unknown/missing fields must not fail).
4. **Tier your data by deletability** (config / data / state / cache) and
   put each tier in its XDG home; document that cache deletion is always
   safe.
5. **Corruption tolerance is a feature of the reader, not the writer**:
   every load path has a defined behavior for a bad file (skip one
   contact with a warning; rebuild UI state; drop and re-sync a folder).
6. **Interoperable formats over bespoke ones** wherever a standard
   exists: vCard for contacts (vdir layout), RFC 822 for drafts/outbox,
   TOML for human-edited files, SQLite for structured cache. Bespoke
   binary (rmp-serde) only inside cache-tier blobs.
7. **Version-stamp anything with a schema** (`user_version`, a `version =`
   key in state files) and refuse to open newer-than-known data.
8. **fsync is part of the write**, not an optimization flag — but only on
   the precious tiers; cache writes may skip it (worst case: re-sync).

## 9. What deliberately does NOT persist

- Decoded/rendered message state, thread trees, search results — rebuilt
  from `MailStore`/mail.db on demand.
- Keyring-held secrets never appear in any nitidus file.
- Window size/terminal caps — re-probed every start (terminals change).
- The plurimus/bevy ECS world — never serialized; the resources rebuild
  it, which is the whole point of the reconcile architecture.
